use k8s_openapi::api::core::v1::Node;
use kube::api::{Api, DynamicObject, ListParams};
use kube::core::{ApiResource, GroupVersionKind};
use kube::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Parses a Kubernetes CPU quantity (e.g. "100m", "1", "2500000000n") into
/// millicores. Unparseable or negative input yields 0.
pub fn cpu_millicores(q: &str) -> u64 {
    crate::quantity::cpu_millis(q)
        .unwrap_or(0)
        .try_into()
        .unwrap_or(0)
}

/// Parses a Kubernetes memory quantity (e.g. "128Mi", "1Gi", bytes) into
/// mebibytes. Unparseable or negative input yields 0.
pub fn mem_mib(q: &str) -> u64 {
    crate::quantity::mem_mib(q)
        .unwrap_or(0)
        .try_into()
        .unwrap_or(0)
}

#[derive(Debug, Deserialize)]
pub struct RawPodMetrics {
    pub metadata: RawMetaName,
    #[serde(default)]
    pub containers: Vec<RawContainerUsage>,
}

#[derive(Debug, Deserialize)]
pub struct RawMetaName {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawContainerUsage {
    #[serde(default)]
    pub usage: RawUsage,
}

#[derive(Debug, Default, Deserialize)]
pub struct RawUsage {
    #[serde(default)]
    pub cpu: String,
    #[serde(default)]
    pub memory: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetricsRow {
    pub key: String,
    pub name: String,
    pub namespace: Option<String>,
    pub cpu_millicores: u64,
    pub mem_mib: u64,
}

pub fn pod_metrics_row(pm: &RawPodMetrics) -> MetricsRow {
    let name = pm.metadata.name.clone().unwrap_or_default();
    let namespace = pm.metadata.namespace.clone();
    let cpu = pm
        .containers
        .iter()
        .map(|c| cpu_millicores(&c.usage.cpu))
        .sum();
    let mem = pm.containers.iter().map(|c| mem_mib(&c.usage.memory)).sum();
    let key = match &namespace {
        Some(ns) if !ns.is_empty() => format!("{ns}/{name}"),
        _ => name.clone(),
    };
    MetricsRow {
        key,
        name,
        namespace,
        cpu_millicores: cpu,
        mem_mib: mem,
    }
}

/// List pod metrics via metrics.k8s.io. Returns empty (not error) when
/// metrics-server is absent (404/NotFound/ServiceUnavailable) so the UI
/// degrades gracefully instead of surfacing an error.
pub async fn pod_metrics(
    client: Client,
    namespace: Option<&str>,
) -> Result<Vec<MetricsRow>, String> {
    let ar = ApiResource::from_gvk(&GroupVersionKind {
        group: "metrics.k8s.io".into(),
        version: "v1beta1".into(),
        kind: "PodMetrics".into(),
    });
    let api: Api<DynamicObject> = match namespace {
        Some(ns) if !ns.is_empty() => Api::namespaced_with(client, ns, &ar),
        _ => Api::all_with(client, &ar),
    };
    match api.list(&ListParams::default()).await {
        Ok(list) => {
            let mut rows = Vec::new();
            for obj in list.items {
                let v = serde_json::to_value(&obj).map_err(|e| e.to_string())?;
                if let Ok(pm) = serde_json::from_value::<RawPodMetrics>(v) {
                    rows.push(pod_metrics_row(&pm));
                }
            }
            rows.sort_by(|a, b| a.key.cmp(&b.key));
            Ok(rows)
        }
        Err(e) => empty_if_metrics_absent(e),
    }
}

/// metrics-server absent: the metrics.k8s.io group/resource itself is
/// unregistered, which the apiserver reports as a 404 ErrorResponse
/// (kube::Error::Api(ErrorResponse { code, .. }), code is a u16). Belt-and-
/// suspenders: some clusters/proxies surface the missing API group as a
/// different error shape (e.g. a transport-level ServiceUnavailable) rather
/// than a clean 404 ErrorResponse.
fn empty_if_metrics_absent<T>(e: kube::Error) -> Result<Vec<T>, String> {
    if let kube::Error::Api(ae) = &e {
        if ae.code == 404 {
            return Ok(Vec::new());
        }
    }
    let s = e.to_string();
    if s.contains("404") || s.contains("NotFound") || s.contains("ServiceUnavailable") {
        Ok(Vec::new())
    } else {
        Err(s)
    }
}

#[derive(Debug, Deserialize)]
pub struct RawNodeMetrics {
    pub metadata: RawMetaName,
    #[serde(default)]
    pub usage: RawUsage,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeMetricsRow {
    pub name: String,
    pub cpu_millicores: u64,
    /// `None` when the node's allocatable capacity could not be read, so the
    /// UI omits the percentage instead of showing a bogus one.
    pub cpu_allocatable_millicores: Option<u64>,
    pub mem_mib: u64,
    pub mem_allocatable_mib: Option<u64>,
}

/// Node allocatable CPU (millicores) / memory (MiB) keyed by node name.
/// Failures degrade to an empty map: usage still renders, percentages don't.
async fn node_allocatable(client: Client) -> BTreeMap<String, (Option<u64>, Option<u64>)> {
    let api: Api<Node> = Api::all(client);
    let Ok(list) = api.list(&ListParams::default()).await else {
        return BTreeMap::new();
    };
    list.items
        .into_iter()
        .filter_map(|n| {
            let name = n.metadata.name?;
            let alloc = n.status.and_then(|s| s.allocatable);
            let get = |key: &str| {
                alloc
                    .as_ref()
                    .and_then(|a| a.get(key))
                    .map(|q| q.0.as_str())
            };
            Some((
                name,
                (get("cpu").map(cpu_millicores), get("memory").map(mem_mib)),
            ))
        })
        .collect()
}

/// List node usage via metrics.k8s.io, joined with Node allocatable. Returns
/// empty (not error) when metrics-server is absent, like [`pod_metrics`].
pub async fn node_metrics(client: Client) -> Result<Vec<NodeMetricsRow>, String> {
    let ar = ApiResource::from_gvk(&GroupVersionKind {
        group: "metrics.k8s.io".into(),
        version: "v1beta1".into(),
        kind: "NodeMetrics".into(),
    });
    let api: Api<DynamicObject> = Api::all_with(client.clone(), &ar);
    let list = match api.list(&ListParams::default()).await {
        Ok(list) => list,
        Err(e) => return empty_if_metrics_absent(e),
    };
    if list.items.is_empty() {
        return Ok(Vec::new());
    }
    let allocatable = node_allocatable(client).await;
    let mut rows = Vec::new();
    for obj in list.items {
        let v = serde_json::to_value(&obj).map_err(|e| e.to_string())?;
        let Ok(nm) = serde_json::from_value::<RawNodeMetrics>(v) else {
            continue;
        };
        let name = nm.metadata.name.unwrap_or_default();
        let (cpu_alloc, mem_alloc) = allocatable.get(&name).copied().unwrap_or((None, None));
        rows.push(NodeMetricsRow {
            name,
            cpu_millicores: cpu_millicores(&nm.usage.cpu),
            cpu_allocatable_millicores: cpu_alloc,
            mem_mib: mem_mib(&nm.usage.memory),
            mem_allocatable_mib: mem_alloc,
        });
    }
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cpu_quantities() {
        assert_eq!(cpu_millicores("100m"), 100);
        assert_eq!(cpu_millicores("1"), 1000);
        assert_eq!(cpu_millicores("250m"), 250);
        assert_eq!(cpu_millicores("2500000000n"), 2500); // nanocores
        assert_eq!(cpu_millicores(""), 0);
        assert_eq!(cpu_millicores("garbage"), 0);
    }

    #[test]
    fn parses_memory_quantities_to_mib() {
        assert_eq!(mem_mib("128Mi"), 128);
        assert_eq!(mem_mib("1Gi"), 1024);
        assert_eq!(mem_mib("1024Ki"), 1);
        assert_eq!(mem_mib("0"), 0);
        assert_eq!(mem_mib("garbage"), 0);
    }

    #[test]
    fn parses_fractional_large_memory() {
        assert_eq!(mem_mib("1.5Gi"), 1536);
        assert_eq!(mem_mib("2Gi"), 2048);
        assert_eq!(mem_mib("0.5Ti"), 512 * 1024);
    }

    #[test]
    fn maps_pod_metrics_summing_containers() {
        let json = serde_json::json!({
            "metadata": {"name": "web", "namespace": "app"},
            "containers": [
                {"name": "a", "usage": {"cpu": "100m", "memory": "64Mi"}},
                {"name": "b", "usage": {"cpu": "50m", "memory": "32Mi"}}
            ]
        });
        let pm: RawPodMetrics = serde_json::from_value(json).unwrap();
        let row = pod_metrics_row(&pm);
        assert_eq!(row.key, "app/web");
        assert_eq!(row.cpu_millicores, 150);
        assert_eq!(row.mem_mib, 96);
    }

    async fn kind_session() -> crate::session::ClusterSession {
        let paths = kxs_core::kubeconfig::paths::kubeconfig_paths();
        let store = kxs_core::kubeconfig::store::KubeconfigStore::load(paths).unwrap();
        let yaml = crate::bridge::kubeconfig_yaml_for_context(&store, "kind-local").unwrap();
        crate::session::connect(&yaml, "kind-local").await.unwrap()
    }

    /// Run manually: cargo test -p kxs-cluster -- --ignored (needs kind-local
    /// in ~/.kube/config). kind clusters typically have no metrics-server, so
    /// the expected outcome is `Ok(vec![])`; if metrics-server happens to be
    /// installed, assert the rows are well-formed instead. Either way this
    /// must not return an `Err`.
    #[tokio::test]
    #[ignore]
    async fn pod_metrics_degrades_gracefully_without_metrics_server() {
        let session = kind_session().await;
        let rows = pod_metrics(session.client.clone(), Some("kube-system"))
            .await
            .expect("pod_metrics must not error even when metrics-server is absent");

        if rows.is_empty() {
            eprintln!(
                "pod_metrics: no metrics-server present, got empty rows (expected on kind-local)"
            );
        } else {
            eprintln!(
                "pod_metrics: metrics-server present, got {} rows",
                rows.len()
            );
            for row in &rows {
                assert!(!row.key.is_empty());
                assert!(!row.name.is_empty());
            }
        }
    }

    /// Same contract as the pod_metrics test above: run manually against
    /// kind-local, must never return an `Err`.
    #[tokio::test]
    #[ignore]
    async fn node_metrics_degrades_gracefully_without_metrics_server() {
        let session = kind_session().await;
        let rows = node_metrics(session.client.clone())
            .await
            .expect("node_metrics must not error even when metrics-server is absent");

        if rows.is_empty() {
            eprintln!(
                "node_metrics: no metrics-server present, got empty rows (expected on kind-local)"
            );
        } else {
            eprintln!(
                "node_metrics: metrics-server present, got {} rows",
                rows.len()
            );
            for row in &rows {
                assert!(!row.name.is_empty());
                if let Some(alloc) = row.cpu_allocatable_millicores {
                    assert!(alloc > 0, "allocatable cpu should be positive when known");
                }
            }
        }
    }
}
