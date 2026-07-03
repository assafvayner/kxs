use kube::api::{Api, DynamicObject, ListParams};
use kube::core::{ApiResource, GroupVersionKind};
use kube::Client;
use serde::{Deserialize, Serialize};

/// Parses a Kubernetes CPU quantity (e.g. "100m", "1", "2500000000n") into
/// millicores. Unparseable input yields 0.
pub fn cpu_millicores(q: &str) -> u64 {
    let q = q.trim();
    if q.is_empty() {
        return 0;
    }
    if let Some(n) = q.strip_suffix('n') {
        return n.parse::<u64>().unwrap_or(0) / 1_000_000;
    }
    if let Some(u) = q.strip_suffix('u') {
        return u.parse::<u64>().unwrap_or(0) / 1_000;
    }
    if let Some(m) = q.strip_suffix('m') {
        return m.parse::<u64>().unwrap_or(0);
    }
    // cores -> millicores
    q.parse::<f64>().map(|c| (c * 1000.0) as u64).unwrap_or(0)
}

/// Parses a Kubernetes memory quantity (e.g. "128Mi", "1Gi", bytes) into
/// mebibytes. Unparseable input yields 0.
pub fn mem_mib(q: &str) -> u64 {
    let q = q.trim();
    if q.is_empty() {
        return 0;
    }
    if let Some(n) = q.strip_suffix("Ki") {
        return n.parse::<u64>().unwrap_or(0) / 1024;
    }
    if let Some(n) = q.strip_suffix("Mi") {
        return n.parse::<u64>().unwrap_or(0);
    }
    if let Some(n) = q.strip_suffix("Gi") {
        return n.parse::<f64>().map(|g| (g * 1024.0) as u64).unwrap_or(0);
    }
    if let Some(n) = q.strip_suffix("Ti") {
        return n
            .parse::<f64>()
            .map(|t| (t * 1024.0 * 1024.0) as u64)
            .unwrap_or(0);
    }
    // bytes
    q.parse::<u64>().unwrap_or(0) / (1024 * 1024)
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
        // metrics-server absent: the metrics.k8s.io group/resource itself is
        // unregistered, which the apiserver reports as a 404 ErrorResponse
        // (kube::Error::Api(ErrorResponse { code, .. }), code is a u16).
        Err(kube::Error::Api(ae)) if ae.code == 404 => Ok(Vec::new()),
        Err(e) => {
            // Belt-and-suspenders: some clusters/proxies surface the missing
            // API group as a different error shape (e.g. a transport-level
            // ServiceUnavailable) rather than a clean 404 ErrorResponse.
            let s = e.to_string();
            if s.contains("404") || s.contains("NotFound") || s.contains("ServiceUnavailable") {
                Ok(Vec::new())
            } else {
                Err(s)
            }
        }
    }
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
}
