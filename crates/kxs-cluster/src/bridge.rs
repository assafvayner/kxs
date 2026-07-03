use kxs_core::kubeconfig::store::KubeconfigStore;
use kxs_core::kubeconfig::types::Kubeconfig;

/// Minimal single-context kubeconfig YAML handed to kube-rs. Extracting only
/// the referenced entries keeps unrelated credentials out of the session.
pub fn kubeconfig_yaml_for_context(
    store: &KubeconfigStore,
    context: &str,
) -> Result<String, String> {
    let (_, nc) = store
        .find_context(context)
        .ok_or_else(|| format!("context \"{context}\" not found"))?;
    let (_, cluster) = store
        .find_cluster(&nc.context.cluster)
        .ok_or_else(|| format!("cluster \"{}\" not found", nc.context.cluster))?;
    let (_, user) = store
        .find_user(&nc.context.user)
        .ok_or_else(|| format!("user \"{}\" not found", nc.context.user))?;
    let kc = Kubeconfig {
        clusters: vec![cluster.clone()],
        users: vec![user.clone()],
        contexts: vec![nc.clone()],
        current_context: Some(context.to_string()),
        ..Default::default()
    };
    serde_yaml_ng::to_string(&kc).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kxs_core::kubeconfig::store::KubeconfigStore;
    use std::path::PathBuf;

    const FIXTURE: &str = r#"
current-context: prod
clusters:
  - {name: prod, cluster: {server: "https://prod.example.com", certificate-authority-data: LS0tCg==}}
  - {name: other, cluster: {server: "https://other"}}
users:
  - name: prod-user
    user:
      exec:
        apiVersion: client.authentication.k8s.io/v1beta1
        command: aws
        args: ["eks", "get-token", "--cluster-name", "prod"]
  - {name: other-user, user: {token: t}}
contexts:
  - {name: prod, context: {cluster: prod, user: prod-user, namespace: scanner}}
  - {name: other, context: {cluster: other, user: other-user}}
"#;

    fn store(dir: &tempfile::TempDir) -> KubeconfigStore {
        let p: PathBuf = dir.path().join("config");
        std::fs::write(&p, FIXTURE).unwrap();
        KubeconfigStore::load(vec![p]).unwrap()
    }

    #[test]
    fn extracts_single_context() {
        let dir = tempfile::tempdir().unwrap();
        let yaml = kubeconfig_yaml_for_context(&store(&dir), "prod").unwrap();
        assert!(yaml.contains("https://prod.example.com"));
        assert!(yaml.contains("get-token"));
        assert!(yaml.contains("current-context: prod"));
        assert!(
            !yaml.contains("https://other"),
            "unrelated entries leaked: {yaml}"
        );
    }

    #[test]
    fn kube_parses_extracted_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let yaml = kubeconfig_yaml_for_context(&store(&dir), "prod").unwrap();
        let kc = kube::config::Kubeconfig::from_yaml(&yaml).unwrap();
        assert_eq!(kc.current_context.as_deref(), Some("prod"));
        assert_eq!(kc.contexts.len(), 1);
        assert_eq!(
            kc.contexts[0]
                .context
                .as_ref()
                .unwrap()
                .namespace
                .as_deref(),
            Some("scanner")
        );
    }

    #[test]
    fn unknown_context_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(kubeconfig_yaml_for_context(&store(&dir), "nope").is_err());
    }

    #[test]
    fn missing_cluster_ref_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config");
        std::fs::write(
            &p,
            "contexts: [{name: broken, context: {cluster: gone, user: gone}}]\n",
        )
        .unwrap();
        let store = KubeconfigStore::load(vec![p]).unwrap();
        let err = kubeconfig_yaml_for_context(&store, "broken").unwrap_err();
        assert!(err.contains("gone"));
    }
}
