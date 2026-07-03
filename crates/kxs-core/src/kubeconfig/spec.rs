use crate::error::{Error, Result};
use crate::kubeconfig::store::KubeconfigStore;
use crate::kubeconfig::types::*;
use serde::Deserialize;
use std::path::PathBuf;

pub const DEFAULT_EXEC_API_VERSION: &str = "client.authentication.k8s.io/v1beta1";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSpec {
    pub name: String,
    /// Some(..) => editing that context; None => creating.
    #[serde(default)]
    pub original_name: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub target_file: Option<String>,
    pub cluster: ClusterSpec,
    pub user: UserSpec,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSpec {
    #[serde(default)]
    pub existing: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub ca_file: Option<String>,
    #[serde(default)]
    pub ca_data: Option<String>,
    #[serde(default)]
    pub insecure_skip_tls_verify: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSpec {
    #[serde(default)]
    pub existing: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub client_certificate: Option<String>,
    #[serde(default)]
    pub client_key: Option<String>,
    #[serde(default)]
    pub exec_command: Option<String>,
    #[serde(default)]
    pub exec_args: Option<Vec<String>>,
    #[serde(default)]
    pub exec_env: Option<Vec<[String; 2]>>,
    #[serde(default)]
    pub exec_api_version: Option<String>,
}

pub fn apply_context_spec(store: &mut KubeconfigStore, spec: ContextSpec) -> Result<()> {
    let target = spec.target_file.as_ref().map(PathBuf::from);

    let cluster_name = match (&spec.cluster.existing, &spec.cluster.name) {
        (Some(existing), _) => existing.clone(),
        (None, Some(new_name)) => {
            let cluster = Cluster {
                server: spec.cluster.server.clone(),
                certificate_authority: spec.cluster.ca_file.clone(),
                certificate_authority_data: spec.cluster.ca_data.clone(),
                insecure_skip_tls_verify: spec.cluster.insecure_skip_tls_verify,
                extras: Extras::new(),
            };
            store.upsert_cluster(new_name, cluster, target.as_deref())?;
            new_name.clone()
        }
        (None, None) => {
            return Err(Error::NotFound {
                kind: "cluster",
                name: "<unspecified>".into(),
            })
        }
    };

    let user_name = match (&spec.user.existing, &spec.user.name) {
        (Some(existing), _) => existing.clone(),
        (None, Some(new_name)) => {
            let exec = spec.user.exec_command.as_ref().map(|cmd| ExecConfig {
                api_version: spec
                    .user
                    .exec_api_version
                    .clone()
                    .unwrap_or_else(|| DEFAULT_EXEC_API_VERSION.into()),
                command: cmd.clone(),
                args: spec.user.exec_args.clone().unwrap_or_default(),
                env: spec.user.exec_env.clone().map(|pairs| {
                    pairs
                        .into_iter()
                        .map(|[name, value]| ExecEnv {
                            name,
                            value,
                            extras: Extras::new(),
                        })
                        .collect()
                }),
                extras: Extras::new(),
            });
            let user = AuthInfo {
                token: spec.user.token.clone(),
                client_certificate: spec.user.client_certificate.clone(),
                client_key: spec.user.client_key.clone(),
                exec,
                ..Default::default()
            };
            store.upsert_user(new_name, user, target.as_deref())?;
            new_name.clone()
        }
        (None, None) => {
            return Err(Error::NotFound {
                kind: "user",
                name: "<unspecified>".into(),
            })
        }
    };

    match &spec.original_name {
        None => store.create_context(
            &spec.name,
            &cluster_name,
            &user_name,
            spec.namespace.clone(),
            target.as_deref(),
        ),
        Some(orig) => {
            if orig != &spec.name {
                store.rename_context(orig, &spec.name)?;
            }
            store.update_context(
                &spec.name,
                &cluster_name,
                &user_name,
                spec.namespace.clone(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kubeconfig::store::KubeconfigStore;

    const BASE: &str = r#"
clusters: [{name: prod, cluster: {server: "https://a"}}]
users: [{name: u1, user: {token: t}}]
contexts: [{name: prod, context: {cluster: prod, user: u1}}]
"#;

    fn setup(dir: &tempfile::TempDir) -> (KubeconfigStore, std::path::PathBuf) {
        let p = dir.path().join("config");
        std::fs::write(&p, BASE).unwrap();
        (KubeconfigStore::load(vec![p.clone()]).unwrap(), p)
    }

    #[test]
    fn create_with_existing_refs() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = setup(&dir);
        let spec: ContextSpec = serde_json::from_str(
            r#"{"name":"prod-ro","namespace":"scanner",
                "cluster":{"existing":"prod"},"user":{"existing":"u1"}}"#,
        )
        .unwrap();
        apply_context_spec(&mut store, spec).unwrap();
        let ctx = store
            .contexts()
            .into_iter()
            .find(|c| c.name == "prod-ro")
            .unwrap();
        assert_eq!(ctx.cluster, "prod");
        assert_eq!(ctx.namespace.as_deref(), Some("scanner"));
    }

    #[test]
    fn create_with_new_cluster_and_exec_user() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, path) = setup(&dir);
        let spec: ContextSpec = serde_json::from_str(
            r#"{"name":"eks","cluster":{"name":"eks-c","server":"https://eks","caData":"LS0tCg=="},
                "user":{"name":"eks-u","execCommand":"aws",
                        "execArgs":["eks","get-token","--cluster-name","eks-c"],
                        "execEnv":[["AWS_PROFILE","prod"]]}}"#,
        )
        .unwrap();
        apply_context_spec(&mut store, spec).unwrap();
        let (_, user) = store.find_user("eks-u").unwrap();
        let exec = user.user.exec.as_ref().unwrap();
        assert_eq!(exec.command, "aws");
        assert_eq!(exec.api_version, "client.authentication.k8s.io/v1beta1");
        assert_eq!(exec.env.as_ref().unwrap()[0].name, "AWS_PROFILE");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("eks-c"));
    }

    #[test]
    fn edit_renames_and_rebinds() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = setup(&dir);
        let spec: ContextSpec = serde_json::from_str(
            r#"{"name":"production","originalName":"prod","namespace":"web",
                "cluster":{"existing":"prod"},"user":{"existing":"u1"}}"#,
        )
        .unwrap();
        apply_context_spec(&mut store, spec).unwrap();
        assert!(store.find_context("prod").is_none());
        let ctx = store
            .contexts()
            .into_iter()
            .find(|c| c.name == "production")
            .unwrap();
        assert_eq!(ctx.namespace.as_deref(), Some("web"));
    }

    #[test]
    fn unknown_existing_cluster_errors() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = setup(&dir);
        let spec: ContextSpec = serde_json::from_str(
            r#"{"name":"x","cluster":{"existing":"nope"},"user":{"existing":"u1"}}"#,
        )
        .unwrap();
        assert!(apply_context_spec(&mut store, spec).is_err());
    }
}
