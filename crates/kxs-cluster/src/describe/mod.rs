//! `kubectl describe` output for any object, in Rust.
//!
//! `describe` fetches the object and best-effort related objects, then
//! `describe_value` (pure) formats them. Typed describers handle the common
//! built-in kinds; everything else falls back to `generic`.

pub mod events;
pub mod generic;
pub mod header;
pub mod pod;
pub mod util;
pub mod workloads;
pub mod writer;

use crate::discovery::ResourceKind;
use crate::resources::{api_resource, get_events, ResourceEvent};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet, StatefulSet};
use k8s_openapi::api::core::v1::{Endpoints, LimitRange, Pod, ResourceQuota, Secret};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use k8s_openapi::chrono::{DateTime, Utc};
use kube::api::{Api, DynamicObject, ListParams};
use kube::Client;
use serde::de::DeserializeOwned;
use serde_json::Value;
use writer::Writer;

/// Related objects some describers print (all best-effort, empty on error).
#[derive(Debug, Default)]
pub struct Lookups {
    /// Service: its Endpoints object.
    pub endpoints: Option<Endpoints>,
    /// Node: pods scheduled on it. ReplicaSet/StatefulSet/DaemonSet: pods matching the selector.
    /// PersistentVolumeClaim: pods in the namespace (filtered by claim name when printing).
    pub pods: Vec<Pod>,
    /// Deployment: ReplicaSets matching its selector.
    pub replica_sets: Vec<ReplicaSet>,
    /// Namespace: quotas and limit ranges in it.
    pub quotas: Vec<ResourceQuota>,
    pub limit_ranges: Vec<LimitRange>,
    /// ServiceAccount: token secrets in the namespace.
    pub secrets: Vec<Secret>,
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
    let _ = now_ms;
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
            l.secrets = list_or_empty(
                Api::<Secret>::namespaced(client.clone(), ns),
                ListParams::default().fields("type=kubernetes.io/service-account-token"),
            )
            .await;
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
