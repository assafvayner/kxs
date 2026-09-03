use k8s_openapi::api::apps::v1::{Deployment, ReplicaSet};
use k8s_openapi::api::batch::v1::{CronJob, Job};
use k8s_openapi::api::core::v1::{ConfigMap, Endpoints, Pod, Secret, Service};
use kube::api::{Api, DynamicObject, EvictParams, ListParams, ObjectMeta, PostParams};
use kube::Client;
use serde::Serialize;

/// Names of the pods selected by a workload's `.spec.selector`, sorted.
/// Only `matchLabels` (or a Service-style bare label map) is honored;
/// matchExpressions-only selectors are rejected rather than over-matched.
pub async fn workload_pods(
    client: Client,
    group: &str,
    version: &str,
    kind: &str,
    plural: &str,
    namespace: &str,
    name: &str,
) -> Result<Vec<String>, String> {
    let ar = crate::resources::api_resource(group, version, kind, plural);
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &ar);
    let obj = api
        .get_opt(name)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{kind} \"{name}\" not found"))?;
    let selector = selector_from_spec(&obj.data)
        .ok_or_else(|| format!("{kind} \"{name}\" has no label selector"))?;
    let pods: Api<Pod> = Api::namespaced(client, namespace);
    let list = pods
        .list(&ListParams::default().labels(&selector))
        .await
        .map_err(|e| e.to_string())?;
    let mut names: Vec<String> = list
        .items
        .into_iter()
        .filter_map(|p| p.metadata.name)
        .collect();
    names.sort();
    Ok(names)
}

/// The label selector string of a workload ("k1=v1,k2=v2"), for driving a pod
/// watch from a pod-owner row. Errors when the workload has no usable selector.
pub async fn workload_selector(
    client: Client,
    group: &str,
    version: &str,
    kind: &str,
    plural: &str,
    namespace: &str,
    name: &str,
) -> Result<String, String> {
    let ar = crate::resources::api_resource(group, version, kind, plural);
    let api: Api<DynamicObject> = Api::namespaced_with(client, namespace, &ar);
    let obj = api
        .get_opt(name)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{kind} \"{name}\" not found"))?;
    selector_from_spec(&obj.data).ok_or_else(|| format!("{kind} \"{name}\" has no label selector"))
}

/// "k1=v1,k2=v2" from `.spec.selector.matchLabels`, or from a bare label map
/// (`.spec.selector` on Services). None when there is no usable label map.
fn selector_from_spec(data: &serde_json::Value) -> Option<String> {
    let sel = &data["spec"]["selector"];
    let map = sel
        .get("matchLabels")
        .and_then(|m| m.as_object())
        .or_else(|| {
            sel.as_object()
                .filter(|m| m.values().all(|v| v.is_string()))
        })?;
    if map.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = map
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|v| format!("{k}={v}")))
        .collect();
    parts.sort();
    Some(parts.join(","))
}

const REVISION_ANNOTATION: &str = "deployment.kubernetes.io/revision";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RolloutRevision {
    pub revision: i64,
    /// Name of the ReplicaSet backing this revision.
    pub name: String,
    pub created: Option<String>,
    pub images: Vec<String>,
    pub replicas: i32,
    pub current: bool,
}

async fn deployment_replicasets(
    client: Client,
    namespace: &str,
    name: &str,
) -> Result<(Deployment, Vec<(i64, ReplicaSet)>), String> {
    let deps: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let dep = deps.get(name).await.map_err(|e| e.to_string())?;
    let uid = dep.metadata.uid.clone().ok_or("deployment has no uid")?;
    let rss: Api<ReplicaSet> = Api::namespaced(client, namespace);
    let list = rss
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;
    let owned = list
        .items
        .into_iter()
        .filter_map(|rs| {
            let is_owned = rs
                .metadata
                .owner_references
                .as_ref()
                .is_some_and(|os| os.iter().any(|o| o.uid == uid));
            if !is_owned {
                return None;
            }
            let rev: i64 = rs
                .metadata
                .annotations
                .as_ref()?
                .get(REVISION_ANNOTATION)?
                .parse()
                .ok()?;
            Some((rev, rs))
        })
        .collect();
    Ok((dep, owned))
}

/// Revision history of a Deployment, newest first, from its owned ReplicaSets
/// (the same source `kubectl rollout history` reads).
pub async fn rollout_history(
    client: Client,
    namespace: &str,
    name: &str,
) -> Result<Vec<RolloutRevision>, String> {
    let (dep, owned) = deployment_replicasets(client, namespace, name).await?;
    let current_rev = dep
        .metadata
        .annotations
        .as_ref()
        .and_then(|a| a.get(REVISION_ANNOTATION))
        .cloned();
    let mut out: Vec<RolloutRevision> = owned
        .into_iter()
        .map(|(rev, rs)| {
            let images = rs
                .spec
                .as_ref()
                .and_then(|s| s.template.as_ref())
                .and_then(|t| t.spec.as_ref())
                .map(|ps| {
                    ps.containers
                        .iter()
                        .filter_map(|c| c.image.clone())
                        .collect()
                })
                .unwrap_or_default();
            RolloutRevision {
                revision: rev,
                name: rs.metadata.name.clone().unwrap_or_default(),
                created: rs
                    .metadata
                    .creation_timestamp
                    .as_ref()
                    .map(|t| t.0.to_rfc3339()),
                images,
                replicas: rs.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0),
                current: current_rev.as_deref() == Some(rev.to_string().as_str()),
            }
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.revision));
    Ok(out)
}

/// Roll the Deployment's pod template back to the given revision's ReplicaSet
/// template (what `kubectl rollout undo --to-revision` does). Uses a full
/// replace so fields removed since that revision are actually removed.
pub async fn rollout_undo(
    client: Client,
    namespace: &str,
    name: &str,
    revision: i64,
) -> Result<(), String> {
    let (mut dep, owned) = deployment_replicasets(client.clone(), namespace, name).await?;
    let (_, rs) = owned
        .into_iter()
        .find(|(rev, _)| *rev == revision)
        .ok_or_else(|| format!("revision {revision} not found for {name}"))?;
    let mut template = rs
        .spec
        .and_then(|s| s.template)
        .ok_or("revision has no pod template")?;
    if let Some(labels) = template.metadata.as_mut().and_then(|m| m.labels.as_mut()) {
        labels.remove("pod-template-hash");
    }
    dep.spec.as_mut().ok_or("deployment has no spec")?.template = template;
    dep.metadata.managed_fields = None;
    let deps: Api<Deployment> = Api::namespaced(client, namespace);
    deps.replace(name, &PostParams::default(), &dep)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrainReport {
    pub evicted: u32,
    pub skipped: u32,
    pub failed: Vec<String>,
}

/// DaemonSet-owned and mirror (static) pods survive a drain.
fn drain_skips(pod: &Pod) -> bool {
    let meta = &pod.metadata;
    let daemonset = meta
        .owner_references
        .as_ref()
        .is_some_and(|os| os.iter().any(|o| o.kind == "DaemonSet"));
    let mirror = meta
        .annotations
        .as_ref()
        .is_some_and(|a| a.contains_key("kubernetes.io/config.mirror"));
    daemonset || mirror
}

/// Cordon the node, then evict every pod on it except DaemonSet-owned and
/// mirror pods. Evictions go through the Eviction API, so PodDisruptionBudget
/// refusals are reported per pod rather than bypassed.
pub async fn drain_node(client: Client, name: &str) -> Result<DrainReport, String> {
    crate::edit::merge_patch(
        client.clone(),
        "",
        "v1",
        "Node",
        "nodes",
        None,
        name,
        crate::edit::cordon_patch(true),
    )
    .await?;
    let pods: Api<Pod> = Api::all(client.clone());
    let fields = format!("spec.nodeName={name},status.phase!=Succeeded,status.phase!=Failed");
    let list = pods
        .list(&ListParams::default().fields(&fields))
        .await
        .map_err(|e| e.to_string())?;
    let mut report = DrainReport::default();
    for pod in list.items {
        if drain_skips(&pod) {
            report.skipped += 1;
            continue;
        }
        let pod_name = pod.metadata.name.clone().unwrap_or_default();
        let ns = pod.metadata.namespace.clone().unwrap_or_default();
        let api: Api<Pod> = Api::namespaced(client.clone(), &ns);
        match api.evict(&pod_name, &EvictParams::default()).await {
            Ok(_) => report.evicted += 1,
            Err(e) => report.failed.push(format!("{ns}/{pod_name}: {e}")),
        }
    }
    Ok(report)
}

/// Create a Job from the CronJob's jobTemplate, like
/// `kubectl create job --from=cronjob/<name>`. Returns the created Job's name.
pub async fn trigger_cronjob(
    client: Client,
    namespace: &str,
    name: &str,
) -> Result<String, String> {
    let cjs: Api<CronJob> = Api::namespaced(client.clone(), namespace);
    let cj = cjs.get(name).await.map_err(|e| e.to_string())?;
    let tmpl = cj.spec.ok_or("cronjob has no spec")?.job_template;
    let job_name = manual_job_name(name, unix_now());
    let mut annotations = tmpl
        .metadata
        .as_ref()
        .and_then(|m| m.annotations.clone())
        .unwrap_or_default();
    annotations.insert("cronjob.kubernetes.io/instantiate".into(), "manual".into());
    let job = Job {
        metadata: ObjectMeta {
            name: Some(job_name.clone()),
            namespace: Some(namespace.into()),
            annotations: Some(annotations),
            labels: tmpl.metadata.as_ref().and_then(|m| m.labels.clone()),
            ..Default::default()
        },
        spec: tmpl.spec,
        ..Default::default()
    };
    let jobs: Api<Job> = Api::namespaced(client, namespace);
    jobs.create(&PostParams::default(), &job)
        .await
        .map_err(|e| e.to_string())?;
    Ok(job_name)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `<name>-manual-<suffix>`, with `<name>` truncated so the whole thing fits
/// the 63-char DNS label limit.
fn manual_job_name(cronjob: &str, ts: u64) -> String {
    let suffix = format!("-manual-{}", ts % 100_000_000);
    let base: String = cronjob.chars().take(63 - suffix.len()).collect();
    format!("{}{suffix}", base.trim_end_matches('-'))
}

/// Resolve a Service port to a ready backing (pod, containerPort) via its
/// Endpoints — the target `kubectl port-forward svc/<name>` would pick.
pub async fn resolve_service_endpoint(
    client: Client,
    namespace: &str,
    service: &str,
    service_port: u16,
) -> Result<(String, u16), String> {
    let svcs: Api<Service> = Api::namespaced(client.clone(), namespace);
    let svc = svcs.get(service).await.map_err(|e| e.to_string())?;
    let ports = svc
        .spec
        .as_ref()
        .and_then(|s| s.ports.clone())
        .unwrap_or_default();
    let sp = ports
        .iter()
        .find(|p| p.port == i32::from(service_port))
        .ok_or_else(|| {
            let available: Vec<String> = ports.iter().map(|p| p.port.to_string()).collect();
            format!(
                "service {service} has no port {service_port} (ports: {})",
                available.join(", ")
            )
        })?;
    let port_name = sp.name.clone();
    let eps: Api<Endpoints> = Api::namespaced(client, namespace);
    let ep = eps
        .get(service)
        .await
        .map_err(|e| format!("no endpoints for service {service}: {e}"))?;
    for subset in ep.subsets.unwrap_or_default() {
        let Some(port) = subset
            .ports
            .as_ref()
            .and_then(|ps| ps.iter().find(|p| p.name == port_name))
        else {
            continue;
        };
        let pod = subset.addresses.as_ref().and_then(|addrs| {
            addrs.iter().find_map(|a| {
                let t = a.target_ref.as_ref()?;
                (t.kind.as_deref() == Some("Pod")).then(|| t.name.clone())?
            })
        });
        if let Some(pod) = pod {
            return Ok((
                pod,
                u16::try_from(port.port).map_err(|_| "bad endpoint port")?,
            ));
        }
    }
    Err(format!(
        "service {service} has no ready pod endpoints for port {service_port}"
    ))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEntry {
    pub key: String,
    /// Decoded value; base64 of the raw bytes when `binary`.
    pub value: String,
    pub binary: bool,
}

fn entry_from_bytes(key: String, bytes: &[u8]) -> ConfigEntry {
    match std::str::from_utf8(bytes) {
        Ok(text) => ConfigEntry {
            key,
            value: text.to_string(),
            binary: false,
        },
        Err(_) => {
            use base64::engine::general_purpose::STANDARD;
            use base64::Engine as _;
            ConfigEntry {
                key,
                value: STANDARD.encode(bytes),
                binary: true,
            }
        }
    }
}

/// Decoded data entries of a Secret or ConfigMap, sorted by key.
pub async fn config_values(
    client: Client,
    namespace: &str,
    name: &str,
    kind: &str,
) -> Result<Vec<ConfigEntry>, String> {
    let mut out = Vec::new();
    match kind {
        "Secret" => {
            let api: Api<Secret> = Api::namespaced(client, namespace);
            let s = api.get(name).await.map_err(|e| e.to_string())?;
            for (k, v) in s.data.unwrap_or_default() {
                out.push(entry_from_bytes(k, &v.0));
            }
        }
        "ConfigMap" => {
            let api: Api<ConfigMap> = Api::namespaced(client, namespace);
            let cm = api.get(name).await.map_err(|e| e.to_string())?;
            for (k, v) in cm.data.unwrap_or_default() {
                out.push(ConfigEntry {
                    key: k,
                    value: v,
                    binary: false,
                });
            }
            for (k, v) in cm.binary_data.unwrap_or_default() {
                out.push(entry_from_bytes(k, &v.0));
            }
        }
        other => return Err(format!("values not supported for {other}")),
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn selector_from_match_labels() {
        let spec = json!({"spec": {"selector": {"matchLabels": {"app": "web", "tier": "fe"}}}});
        assert_eq!(
            selector_from_spec(&spec).as_deref(),
            Some("app=web,tier=fe")
        );
    }

    #[test]
    fn selector_from_service_bare_map() {
        let spec = json!({"spec": {"selector": {"app": "web"}}});
        assert_eq!(selector_from_spec(&spec).as_deref(), Some("app=web"));
    }

    #[test]
    fn selector_rejects_expressions_only_and_missing() {
        let exprs = json!({"spec": {"selector": {"matchExpressions": [{"key": "a", "operator": "Exists"}]}}});
        assert_eq!(selector_from_spec(&exprs), None);
        assert_eq!(selector_from_spec(&json!({"spec": {}})), None);
        assert_eq!(
            selector_from_spec(&json!({"spec": {"selector": {"matchLabels": {}}}})),
            None
        );
    }

    #[test]
    fn manual_job_name_fits_dns_label() {
        let n = manual_job_name("digest", 1_756_600_000);
        assert!(n.starts_with("digest-manual-"));
        assert!(n.len() <= 63);
        let long = "x".repeat(80);
        let n = manual_job_name(&long, 1);
        assert!(n.len() <= 63, "got {} chars", n.len());
        assert!(n.ends_with("-manual-1"));
    }

    fn pod_json(v: serde_json::Value) -> Pod {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn drain_skips_daemonset_and_mirror_pods() {
        let ds = pod_json(json!({"metadata": {"name": "p", "ownerReferences": [
            {"apiVersion": "apps/v1", "kind": "DaemonSet", "name": "d", "uid": "u", "controller": true}
        ]}}));
        assert!(drain_skips(&ds));
        let mirror = pod_json(json!({"metadata": {"name": "p", "annotations":
            {"kubernetes.io/config.mirror": "abc"}}}));
        assert!(drain_skips(&mirror));
        let rs_owned = pod_json(json!({"metadata": {"name": "p", "ownerReferences": [
            {"apiVersion": "apps/v1", "kind": "ReplicaSet", "name": "r", "uid": "u"}
        ]}}));
        assert!(!drain_skips(&rs_owned));
        let bare = pod_json(json!({"metadata": {"name": "p"}}));
        assert!(!drain_skips(&bare));
    }

    async fn kind_session() -> crate::session::ClusterSession {
        let paths = kxs_core::kubeconfig::paths::kubeconfig_paths();
        let store = kxs_core::kubeconfig::store::KubeconfigStore::load(paths).unwrap();
        let yaml = crate::bridge::kubeconfig_yaml_for_context(&store, "kind-local").unwrap();
        crate::session::connect(&yaml, "kind-local").await.unwrap()
    }

    /// Run manually: cargo test -p kxs-cluster -- --ignored (needs kind-local).
    #[tokio::test]
    #[ignore]
    async fn workload_pods_finds_coredns_on_kind_local() {
        let s = kind_session().await;
        let pods = workload_pods(
            s.client.clone(),
            "apps",
            "v1",
            "Deployment",
            "deployments",
            "kube-system",
            "coredns",
        )
        .await
        .unwrap();
        assert!(!pods.is_empty(), "coredns deployment should have pods");
        assert!(pods.iter().all(|p| p.starts_with("coredns-")), "{pods:?}");
    }

    #[tokio::test]
    #[ignore]
    async fn rollout_history_lists_coredns_revisions_on_kind_local() {
        let s = kind_session().await;
        let revs = rollout_history(s.client.clone(), "kube-system", "coredns")
            .await
            .unwrap();
        assert!(!revs.is_empty());
        // newest first, and the newest is the current one on an untouched deployment
        assert!(revs.windows(2).all(|w| w[0].revision > w[1].revision));
        assert!(revs[0].current, "{revs:?}");
        assert!(!revs[0].images.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn resolve_service_endpoint_kube_dns_on_kind_local() {
        let s = kind_session().await;
        let (pod, port) = resolve_service_endpoint(s.client.clone(), "kube-system", "kube-dns", 53)
            .await
            .unwrap();
        assert!(pod.starts_with("coredns-"), "{pod}");
        assert_eq!(port, 53);
        // a bogus port errors with the available ports listed
        let err = resolve_service_endpoint(s.client.clone(), "kube-system", "kube-dns", 9999)
            .await
            .unwrap_err();
        assert!(err.contains("no port 9999"), "{err}");
    }

    #[tokio::test]
    #[ignore]
    async fn config_values_reads_root_ca_configmap_on_kind_local() {
        let s = kind_session().await;
        // kube-root-ca.crt exists in every namespace
        let vals = config_values(s.client.clone(), "default", "kube-root-ca.crt", "ConfigMap")
            .await
            .unwrap();
        let ca = vals.iter().find(|e| e.key == "ca.crt").expect("ca.crt key");
        assert!(!ca.binary);
        assert!(ca.value.contains("BEGIN CERTIFICATE"));
    }

    /// A clean namespace for an e2e test. A best-effort create is not enough:
    /// residue from an aborted run, or the same namespace still terminating
    /// from the previous run, makes later creates fail — so delete, wait for
    /// it to disappear, then create fresh.
    async fn fresh_namespace(client: &Client, name: &str) {
        use k8s_openapi::api::core::v1::Namespace;
        use kube::api::DeleteParams;
        let api: Api<k8s_openapi::api::core::v1::Namespace> = Api::all(client.clone());
        let _ = api.delete(name, &DeleteParams::default()).await;
        for _ in 0..120 {
            match api.get_opt(name).await {
                Ok(None) => break,
                _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
            }
        }
        let ns: Namespace = serde_json::from_value(json!({"metadata": {"name": name}})).unwrap();
        api.create(&PostParams::default(), &ns)
            .await
            .expect("create test namespace");
    }

    /// Creates a throwaway CronJob in its own namespace (other tests create
    /// and tear down kxs-e2e concurrently), triggers it, flips suspend, and
    /// cleans up the namespace.
    #[tokio::test]
    #[ignore]
    async fn trigger_and_suspend_cronjob_on_kind_local() {
        use k8s_openapi::api::core::v1::Namespace;
        use kube::api::DeleteParams;
        const NS: &str = "kxs-e2e-cron";
        let s = kind_session().await;
        let client = s.client.clone();

        let ns_api: Api<Namespace> = Api::all(client.clone());
        fresh_namespace(&client, NS).await;

        let cjs: Api<CronJob> = Api::namespaced(client.clone(), NS);
        let cj: CronJob = serde_json::from_value(json!({
            "metadata": {"name": "kxs-cron", "namespace": NS},
            "spec": {
                "schedule": "0 0 1 1 *",
                "jobTemplate": {"spec": {"template": {"spec": {
                    "restartPolicy": "Never",
                    "containers": [{"name": "c", "image": "busybox", "command": ["true"]}]
                }}}}
            }
        }))
        .unwrap();
        cjs.create(&PostParams::default(), &cj).await.unwrap();

        let job_name = trigger_cronjob(client.clone(), NS, "kxs-cron")
            .await
            .unwrap();
        let jobs: Api<Job> = Api::namespaced(client.clone(), NS);
        let job = jobs.get(&job_name).await.expect("triggered job exists");
        assert_eq!(
            job.metadata
                .annotations
                .as_ref()
                .and_then(|a| a.get("cronjob.kubernetes.io/instantiate"))
                .map(String::as_str),
            Some("manual")
        );

        crate::edit::merge_patch(
            client.clone(),
            "batch",
            "v1",
            "CronJob",
            "cronjobs",
            Some(NS),
            "kxs-cron",
            crate::edit::suspend_patch(true),
        )
        .await
        .unwrap();
        let cj = cjs.get("kxs-cron").await.unwrap();
        assert_eq!(cj.spec.and_then(|s| s.suspend), Some(true));

        let _ = ns_api.delete(NS, &DeleteParams::default()).await;
    }

    /// Full undo path: create a deployment, roll a second revision by changing
    /// the image, then roll back to revision 1 and check the image reverted.
    #[tokio::test]
    #[ignore]
    async fn rollout_undo_reverts_image_on_kind_local() {
        use k8s_openapi::api::core::v1::Namespace;
        use kube::api::{DeleteParams, Patch, PatchParams};
        use std::time::Duration;
        const NS: &str = "kxs-e2e-rollout";
        let s = kind_session().await;
        let client = s.client.clone();

        let ns_api: Api<Namespace> = Api::all(client.clone());
        fresh_namespace(&client, NS).await;

        let deps: Api<Deployment> = Api::namespaced(client.clone(), NS);
        let dep: Deployment = serde_json::from_value(json!({
            "metadata": {"name": "kxs-roll", "namespace": NS},
            "spec": {
                "replicas": 1,
                "selector": {"matchLabels": {"app": "kxs-roll"}},
                "template": {
                    "metadata": {"labels": {"app": "kxs-roll"}},
                    "spec": {"containers": [{"name": "c", "image": "nginx:1.25-alpine"}]}
                }
            }
        }))
        .unwrap();
        deps.create(&PostParams::default(), &dep).await.unwrap();

        // second revision: bump the image
        deps.patch(
            "kxs-roll",
            &PatchParams::default(),
            &Patch::Merge(json!({"spec": {"template": {"spec": {"containers": [
                {"name": "c", "image": "nginx:1.27-alpine"}
            ]}}}})),
        )
        .await
        .unwrap();

        // Wait until both revisions' ReplicaSets exist AND the deployment's
        // revision annotation has caught up (it trails the new RS briefly, and
        // `current` is derived from it).
        let mut revs = Vec::new();
        for _ in 0..30 {
            revs = rollout_history(client.clone(), NS, "kxs-roll")
                .await
                .unwrap();
            if revs.len() >= 2 && revs[0].current {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        assert!(revs.len() >= 2, "expected 2 revisions, got {revs:?}");
        assert_eq!(revs[0].images, vec!["nginx:1.27-alpine"]);
        assert_eq!(revs[1].images, vec!["nginx:1.25-alpine"]);
        assert!(revs[0].current && !revs[1].current, "{revs:?}");

        rollout_undo(client.clone(), NS, "kxs-roll", revs[1].revision)
            .await
            .unwrap();
        let dep = deps.get("kxs-roll").await.unwrap();
        let image = dep
            .spec
            .and_then(|s| s.template.spec)
            .map(|ps| ps.containers[0].image.clone().unwrap_or_default())
            .unwrap_or_default();
        assert_eq!(image, "nginx:1.25-alpine");

        let _ = ns_api.delete(NS, &DeleteParams::default()).await;
    }
}
