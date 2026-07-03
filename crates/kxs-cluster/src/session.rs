use k8s_openapi::api::core::v1::Namespace;
use kube::api::ListParams;
use kube::config::KubeConfigOptions;
use kube::{Api, Client};
use std::time::Duration;

pub struct ClusterSession {
    pub context: String,
    pub client: Client,
    pub default_namespace: String,
}

pub async fn connect(kubeconfig_yaml: &str, context: &str) -> Result<ClusterSession, String> {
    let kc = kube::config::Kubeconfig::from_yaml(kubeconfig_yaml).map_err(|e| e.to_string())?;
    let opts = KubeConfigOptions {
        context: Some(context.to_string()),
        ..Default::default()
    };
    let config = kube::Config::from_custom_kubeconfig(kc, &opts)
        .await
        .map_err(|e| e.to_string())?;
    let default_namespace = config.default_namespace.clone();
    let client = Client::try_from(config).map_err(|e| e.to_string())?;
    Ok(ClusterSession {
        context: context.to_string(),
        client,
        default_namespace,
    })
}

/// Version probe with timeout; doubles as the reachability ping.
pub async fn ping(session: &ClusterSession, timeout: Duration) -> Result<String, String> {
    match tokio::time::timeout(timeout, session.client.apiserver_version()).await {
        Ok(Ok(v)) => Ok(v.git_version),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("connection timed out".into()),
    }
}

pub async fn namespaces(session: &ClusterSession) -> Result<Vec<String>, String> {
    let api: Api<Namespace> = Api::all(session.client.clone());
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;
    Ok(list
        .items
        .into_iter()
        .filter_map(|n| n.metadata.name)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run manually: cargo test -p kxs-cluster -- --ignored (needs kind-local in ~/.kube/config)
    #[tokio::test]
    #[ignore]
    async fn connects_to_kind_local() {
        let paths = kxs_core::kubeconfig::paths::kubeconfig_paths();
        let store = kxs_core::kubeconfig::store::KubeconfigStore::load(paths).unwrap();
        let yaml = crate::bridge::kubeconfig_yaml_for_context(&store, "kind-local").unwrap();
        let session = connect(&yaml, "kind-local").await.unwrap();
        let version = ping(&session, Duration::from_secs(5)).await.unwrap();
        assert!(version.starts_with('v'));
        assert!(namespaces(&session)
            .await
            .unwrap()
            .iter()
            .any(|n| n == "default"));
    }
}
