//! `kubectl describe` output for any object, in Rust.
//!
//! `describe` fetches the object and best-effort related objects, then
//! `describe_value` (pure) formats them. Typed describers handle the common
//! built-in kinds; everything else falls back to `generic`.

pub mod batch;
pub mod config;
pub mod events;
pub mod generic;
pub mod header;
pub mod hpa;
pub mod namespace;
pub mod network;
pub mod node;
pub mod pod;
pub mod serviceaccount;
pub mod storage;
pub mod util;
pub mod workloads;
pub mod writer;

use crate::discovery::ResourceKind;
use crate::resources::{api_resource, get_events, ResourceEvent};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet, StatefulSet};
use k8s_openapi::api::autoscaling::v2::HorizontalPodAutoscaler;
use k8s_openapi::api::batch::v1::{CronJob, Job};
use k8s_openapi::api::coordination::v1::Lease;
use k8s_openapi::api::core::v1::{
    ConfigMap, Endpoints, LimitRange, Namespace, Node, PersistentVolume, PersistentVolumeClaim,
    Pod, ResourceQuota, Secret, Service, ServiceAccount,
};
use k8s_openapi::api::networking::v1::Ingress;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use k8s_openapi::chrono::{DateTime, Utc};
use kube::api::{Api, DynamicObject, ListParams};
use kube::Client;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeSet;
use writer::Writer;

#[derive(Debug, Default)]
pub struct ServiceAccountSecretLookup {
    pub existing_names: BTreeSet<String>,
    pub token_metadata: Vec<ObjectMeta>,
}

/// Related objects some describers print (all best-effort, empty or unavailable on error).
#[derive(Debug, Default)]
pub struct Lookups {
    /// Service: its Endpoints object.
    pub endpoints: Option<Endpoints>,
    /// Node: its heartbeat lease from `kube-node-lease`.
    pub lease: Option<Lease>,
    /// Node: pods scheduled on it. ReplicaSet/StatefulSet/DaemonSet: pods matching the selector.
    /// PersistentVolumeClaim: pods in the namespace (filtered by claim name when printing).
    pub pods: Vec<Pod>,
    /// Deployment: ReplicaSets matching its selector.
    pub replica_sets: Vec<ReplicaSet>,
    /// Namespace: quotas and limit ranges in it.
    pub quotas: Vec<ResourceQuota>,
    pub limit_ranges: Vec<LimitRange>,
    /// ServiceAccount: metadata-only Secret lookup. `None` means the lookup failed.
    pub service_account_secrets: Option<ServiceAccountSecretLookup>,
}

/// Pure formatter: object JSON + lookups + events → describe text.
pub fn describe_value(
    kind: &ResourceKind,
    value: &Value,
    lookups: &Lookups,
    events: &[ResourceEvent],
    now: DateTime<Utc>,
) -> String {
    let mut w = Writer::new();
    let wants_events = write_kind(&mut w, kind, value, lookups, now);
    if wants_events {
        events::write_events(&mut w, events, now.timestamp_millis());
    }
    w.finish()
}

/// Dispatch to a typed describer; returns whether an Events section follows.
/// A typed deserialize failure falls through to the generic describer.
fn write_kind(
    w: &mut Writer,
    kind: &ResourceKind,
    value: &Value,
    lookups: &Lookups,
    now: DateTime<Utc>,
) -> bool {
    let now_ms = now.timestamp_millis();
    macro_rules! typed {
        ($t:ty, |$o:ident| $body:expr) => {
            typed!($t, |$o| $body, true)
        };
        ($t:ty, |$o:ident| $body:expr, $events:expr) => {
            if let Ok($o) = serde_json::from_value::<$t>(value.clone()) {
                $body;
                return $events;
            }
        };
    }
    #[allow(clippy::single_match)]
    match (kind.group.as_str(), kind.kind.as_str()) {
        ("", "Pod") => typed!(Pod, |o| pod::write(w, &o)),
        ("apps", "Deployment") => typed!(Deployment, |o| workloads::write_deployment(
            w,
            &o,
            &lookups.replica_sets
        )),
        ("apps", "ReplicaSet") => typed!(ReplicaSet, |o| workloads::write_replicaset(
            w,
            &o,
            &lookups.pods
        )),
        ("apps", "StatefulSet") => typed!(StatefulSet, |o| workloads::write_statefulset(
            w,
            &o,
            &lookups.pods
        )),
        ("apps", "DaemonSet") => typed!(DaemonSet, |o| workloads::write_daemonset(
            w,
            &o,
            &lookups.pods
        )),
        ("batch", "Job") => typed!(Job, |o| batch::write_job(w, &o)),
        ("batch", "CronJob") => typed!(CronJob, |o| batch::write_cronjob(w, &o)),
        ("", "Service") => typed!(Service, |o| network::write_service(
            w,
            &o,
            lookups.endpoints.as_ref()
        )),
        ("", "Endpoints") => typed!(Endpoints, |o| network::write_endpoints(w, &o)),
        ("networking.k8s.io", "Ingress") => typed!(Ingress, |o| network::write_ingress(w, &o)),
        ("", "ConfigMap") => typed!(ConfigMap, |o| config::write_configmap(w, &o)),
        ("", "Secret") => {
            if let Ok(secret) = serde_json::from_value::<Secret>(value.clone()) {
                config::write_secret(w, &secret);
            } else {
                config::write_secret_unstructured(w, value);
            }
            return false;
        }
        ("", "Node") => typed!(Node, |o| node::write(
            w,
            &o,
            lookups.lease.as_ref(),
            &lookups.pods,
            now_ms
        )),
        ("", "Namespace") => {
            if let Ok(namespace) = serde_json::from_value::<Namespace>(value.clone()) {
                namespace::write(w, &namespace, &lookups.quotas, &lookups.limit_ranges);
            } else {
                generic::write(w, value, kind.namespaced);
            }
            return false;
        }
        ("", "PersistentVolumeClaim") => typed!(PersistentVolumeClaim, |o| {
            storage::write_pvc(w, &o, &lookups.pods, now_ms)
        }),
        ("", "PersistentVolume") => {
            typed!(PersistentVolume, |o| { storage::write_pv(w, &o, now_ms) })
        }
        ("", "ServiceAccount") => typed!(ServiceAccount, |o| serviceaccount::write(
            w,
            &o,
            lookups.service_account_secrets.as_ref()
        )),
        ("autoscaling", "HorizontalPodAutoscaler") => {
            typed!(HorizontalPodAutoscaler, |o| hpa::write(w, &o))
        }
        _ => {}
    }
    generic::write(w, value, kind.namespaced);
    true
}

/// Fetch `name`, gather related objects and events, and format.
pub async fn describe(
    client: Client,
    kind: &ResourceKind,
    namespace: Option<&str>,
    name: &str,
) -> Result<String, String> {
    let ar = api_resource(&kind.group, &kind.version, &kind.kind, &kind.plural);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) if !ns.is_empty() && kind.namespaced => {
            Api::namespaced_with(client.clone(), ns, &ar)
        }
        _ => Api::all_with(client.clone(), &ar),
    };
    let obj = api
        .get_opt(name)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{} \"{name}\" not found", kind.kind))?;
    let value = serde_json::to_value(&obj).map_err(|e| e.to_string())?;
    let lookups = gather(&client, kind, namespace, name, &value).await;
    let events = get_events(client, namespace, &kind.kind, name).await;
    Ok(describe_value(kind, &value, &lookups, &events, Utc::now()))
}

async fn list_or_empty<K>(api: Api<K>, lp: ListParams) -> Vec<K>
where
    K: Clone + DeserializeOwned + std::fmt::Debug,
{
    api.list(&lp).await.map(|l| l.items).unwrap_or_default()
}

fn best_effort_get<T, E>(result: Result<Option<T>, E>) -> Option<T> {
    result.ok().flatten()
}

fn selector_of(value: &Value) -> Option<String> {
    let sel: LabelSelector =
        serde_json::from_value(value.get("spec")?.get("selector")?.clone()).ok()?;
    let s = util::selector_string(&sel);
    (s != util::NONE).then_some(s)
}

async fn gather(
    client: &Client,
    kind: &ResourceKind,
    namespace: Option<&str>,
    name: &str,
    value: &Value,
) -> Lookups {
    let mut l = Lookups::default();
    let ns = namespace.unwrap_or("");
    match (kind.group.as_str(), kind.kind.as_str()) {
        ("", "Service") => {
            l.endpoints = Api::<Endpoints>::namespaced(client.clone(), ns)
                .get_opt(name)
                .await
                .ok()
                .flatten();
        }
        ("", "Node") => {
            l.lease = best_effort_get(
                Api::<Lease>::namespaced(client.clone(), "kube-node-lease")
                    .get_opt(name)
                    .await,
            );
            l.pods = list_or_empty(
                Api::<Pod>::all(client.clone()),
                ListParams::default().fields(&format!("spec.nodeName={name}")),
            )
            .await;
        }
        ("", "Namespace") => {
            l.quotas = list_or_empty(
                Api::<ResourceQuota>::namespaced(client.clone(), name),
                ListParams::default(),
            )
            .await;
            l.limit_ranges = list_or_empty(
                Api::<LimitRange>::namespaced(client.clone(), name),
                ListParams::default(),
            )
            .await;
        }
        ("", "PersistentVolumeClaim") => {
            l.pods = list_or_empty(
                Api::<Pod>::namespaced(client.clone(), ns),
                ListParams::default(),
            )
            .await;
        }
        ("", "ServiceAccount") => {
            l.service_account_secrets =
                service_account_secret_lookup(Api::<Secret>::namespaced(client.clone(), ns)).await;
        }
        ("apps", "Deployment") => {
            if let Some(sel) = selector_of(value) {
                l.replica_sets = list_or_empty(
                    Api::<ReplicaSet>::namespaced(client.clone(), ns),
                    ListParams::default().labels(&sel),
                )
                .await;
            }
        }
        ("apps", "ReplicaSet" | "StatefulSet" | "DaemonSet") => {
            if let Some(sel) = selector_of(value) {
                l.pods = list_or_empty(
                    Api::<Pod>::namespaced(client.clone(), ns),
                    ListParams::default().labels(&sel),
                )
                .await;
            }
        }
        _ => {}
    }
    l
}

async fn service_account_secret_lookup(api: Api<Secret>) -> Option<ServiceAccountSecretLookup> {
    let all = api.list_metadata(&ListParams::default()).await.ok()?;
    let tokens = api
        .list_metadata(&ListParams::default().fields("type=kubernetes.io/service-account-token"))
        .await
        .ok()?;
    Some(ServiceAccountSecretLookup {
        existing_names: all
            .items
            .into_iter()
            .filter_map(|secret| secret.metadata.name)
            .collect(),
        token_metadata: tokens
            .items
            .into_iter()
            .map(|secret| secret.metadata)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn best_effort_get_treats_missing_and_failed_lookups_as_absent() {
        assert_eq!(best_effort_get::<Lease, &str>(Ok(None)), None);
        assert_eq!(best_effort_get::<Lease, &str>(Err("forbidden")), None);
    }

    #[test]
    fn malformed_secret_fails_closed_and_suppresses_events() {
        let kind = ResourceKind {
            group: "".into(),
            version: "v1".into(),
            kind: "Secret".into(),
            plural: "secrets".into(),
            namespaced: true,
            aliases: vec![],
        };
        let value = json!({
            "apiVersion": "v1",
            "kind": "Secret",
            "metadata": {"name": "broken", "namespace": "default"},
            "type": "Opaque",
            "immutable": "malformed-optional-secret",
            "data": {
                "invalid": "not-base64-secret!",
                "valid": "c2VjcmV0"
            },
            "stringData": {"password": "plaintext-string-secret"}
        });
        let events = [ResourceEvent {
            type_: "Warning".into(),
            reason: "Leaked".into(),
            message: "event-secret-marker".into(),
            count: 1,
            last_seen: Some("2026-07-03T11:59:00Z".into()),
            first_seen: None,
            source: "secret-source".into(),
        }];

        let output = describe_value(
            &kind,
            &value,
            &Lookups::default(),
            &events,
            "2026-07-03T12:00:00Z".parse().unwrap(),
        );

        assert!(output.contains("Name:         broken\n"));
        assert!(output.contains("Type:  Opaque\n"));
        assert!(output.contains("invalid:  0 bytes\n"));
        assert!(output.contains("valid:  6 bytes\n"));
        for secret in [
            "not-base64-secret!",
            "c2VjcmV0",
            "secret",
            "plaintext-string-secret",
            "malformed-optional-secret",
        ] {
            assert!(!output.contains(secret), "output exposed {secret:?}");
        }
        assert!(!output.contains("Events"));
        assert!(!output.contains("event-secret-marker"));
    }

    #[test]
    fn malformed_namespace_generic_fallback_suppresses_events() {
        let kind = ResourceKind {
            group: "".into(),
            version: "v1".into(),
            kind: "Namespace".into(),
            plural: "namespaces".into(),
            namespaced: false,
            aliases: vec![],
        };
        let value = json!({
            "apiVersion": "v1",
            "kind": "Namespace",
            "metadata": {"name": "broken"},
            "status": {"phase": 42}
        });
        let events = [ResourceEvent {
            type_: "Warning".into(),
            reason: "NamespaceEvent".into(),
            message: "event-namespace-marker".into(),
            count: 1,
            last_seen: Some("2026-07-03T11:59:00Z".into()),
            first_seen: None,
            source: "namespace-source".into(),
        }];

        let output = describe_value(
            &kind,
            &value,
            &Lookups::default(),
            &events,
            "2026-07-03T12:00:00Z".parse().unwrap(),
        );

        assert!(output.contains("Name:         broken\n"));
        assert!(output.contains("Phase:  42\n"));
        assert!(!output.contains("Events"));
        assert!(!output.contains("event-namespace-marker"));
    }

    #[test]
    fn malformed_configmap_generic_fallback_escapes_terminal_controls() {
        let kind = ResourceKind {
            group: "".into(),
            version: "v1".into(),
            kind: "ConfigMap".into(),
            plural: "configmaps".into(),
            namespaced: true,
            aliases: vec![],
        };
        let value = json!({
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": {"name": "broken", "namespace": "default"},
            "immutable": "malformed-configmap",
            "data": {"key\u{1b}\r": "value\u{1b}\r"}
        });

        let output = describe_value(
            &kind,
            &value,
            &Lookups::default(),
            &[],
            "2026-07-03T12:00:00Z".parse().unwrap(),
        );

        assert!(output.contains("API Version:  v1\n"));
        assert!(output.contains("Key^[\\r:  value^[\\r\n"));
        assert!(!output.contains('\u{1b}'));
        assert!(!output.contains('\r'));
    }

    #[test]
    fn typed_secret_escapes_custom_type_and_data_key() {
        let kind = ResourceKind {
            group: "".into(),
            version: "v1".into(),
            kind: "Secret".into(),
            plural: "secrets".into(),
            namespaced: true,
            aliases: vec![],
        };
        let value = json!({
            "apiVersion": "v1",
            "kind": "Secret",
            "metadata": {"name": "safe", "namespace": "default"},
            "type": "custom\u{1b}\r",
            "data": {"key\u{1b}\r": "c2VjcmV0"}
        });

        let output = describe_value(
            &kind,
            &value,
            &Lookups::default(),
            &[],
            "2026-07-03T12:00:00Z".parse().unwrap(),
        );

        assert!(output.contains("Type:  custom^[\\r\n"));
        assert!(output.contains("key^[\\r:  6 bytes\n"));
        assert!(!output.contains('\u{1b}'));
        assert!(!output.contains('\r'));
    }
}
