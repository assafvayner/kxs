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
    pub client_certificate_data: Option<String>,
    #[serde(default)]
    pub client_key_data: Option<String>,
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

    // Validate everything upfront: no disk mutation may happen before all checks pass.
    // A cross-step race can still orphan an upserted cluster/user (accepted; harmless
    // entries, UI refreshes on error).
    if spec.name.trim().is_empty() {
        return Err(Error::Invalid("context name is required".into()));
    }
    if let Some(existing) = &spec.cluster.existing {
        if store.find_cluster(existing).is_none() {
            return Err(Error::NotFound {
                kind: "cluster",
                name: existing.clone(),
            });
        }
    }
    if let Some(existing) = &spec.user.existing {
        if store.find_user(existing).is_none() {
            return Err(Error::NotFound {
                kind: "user",
                name: existing.clone(),
            });
        }
    }
    if let Some(name) = &spec.cluster.name {
        if spec.cluster.existing.is_none() && name.trim().is_empty() {
            return Err(Error::Invalid("cluster name is required".into()));
        }
    }
    if let Some(name) = &spec.user.name {
        if spec.user.existing.is_none() && name.trim().is_empty() {
            return Err(Error::Invalid("user name is required".into()));
        }
    }
    match &spec.original_name {
        None => {
            if store.find_context(&spec.name).is_some() {
                return Err(Error::AlreadyExists {
                    kind: "context",
                    name: spec.name.clone(),
                });
            }
        }
        Some(orig) => {
            if store.find_context(orig).is_none() {
                return Err(Error::NotFound {
                    kind: "context",
                    name: orig.clone(),
                });
            }
            if orig != &spec.name && store.find_context(&spec.name).is_some() {
                return Err(Error::AlreadyExists {
                    kind: "context",
                    name: spec.name.clone(),
                });
            }
        }
    }

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
        (None, None) => return Err(Error::Invalid("no cluster specified".into())),
    };

    let user_name = match (&spec.user.existing, &spec.user.name) {
        (Some(existing), _) => existing.clone(),
        (None, Some(new_name)) => {
            let prev_exec_extras = store
                .find_user(new_name)
                .and_then(|(_, u)| u.user.exec.as_ref())
                .map(|e| e.extras.clone())
                .unwrap_or_default();
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
                extras: prev_exec_extras,
            });
            let user = AuthInfo {
                token: spec.user.token.clone(),
                client_certificate: spec.user.client_certificate.clone(),
                client_key: spec.user.client_key.clone(),
                client_certificate_data: spec.user.client_certificate_data.clone(),
                client_key_data: spec.user.client_key_data.clone(),
                exec,
                ..Default::default()
            };
            store.upsert_user(new_name, user, target.as_deref())?;
            new_name.clone()
        }
        (None, None) => return Err(Error::Invalid("no user specified".into())),
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
    fn edit_with_missing_cluster_fails_before_rename() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = setup(&dir);
        let spec: ContextSpec = serde_json::from_str(
            r#"{"name":"production","originalName":"prod",
                "cluster":{"existing":"nope"},"user":{"existing":"u1"}}"#,
        )
        .unwrap();
        assert!(apply_context_spec(&mut store, spec).is_err());
        assert!(
            store.find_context("prod").is_some(),
            "rename must not have been committed"
        );
        assert!(store.find_context("production").is_none());
    }

    #[test]
    fn create_with_new_cluster_and_missing_user_leaves_no_orphan() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, path) = setup(&dir);
        let spec: ContextSpec = serde_json::from_str(
            r#"{"name":"x","cluster":{"name":"orphan-c","server":"https://o"},
                "user":{"existing":"missing-user"}}"#,
        )
        .unwrap();
        assert!(apply_context_spec(&mut store, spec).is_err());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("orphan-c"),
            "orphaned cluster written: {text}"
        );
    }

    #[test]
    fn empty_names_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = setup(&dir);
        let spec: ContextSpec = serde_json::from_str(
            r#"{"name":"  ","cluster":{"existing":"prod"},"user":{"existing":"u1"}}"#,
        )
        .unwrap();
        assert!(matches!(
            apply_context_spec(&mut store, spec).unwrap_err(),
            crate::error::Error::Invalid(_)
        ));
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

    #[test]
    fn user_inline_cert_data_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = setup(&dir);
        let spec: ContextSpec = serde_json::from_str(
            r#"{"name":"cert-ctx","cluster":{"existing":"prod"},
                "user":{"name":"cert-u","clientCertificateData":"Q0VSVA==","clientKeyData":"S0VZ"}}"#,
        )
        .unwrap();
        apply_context_spec(&mut store, spec).unwrap();
        let (_, user) = store.find_user("cert-u").unwrap();
        assert_eq!(
            user.user.client_certificate_data.as_deref(),
            Some("Q0VSVA==")
        );
        assert_eq!(user.user.client_key_data.as_deref(), Some("S0VZ"));
    }

    #[test]
    fn edit_preserves_exec_extras() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config");
        std::fs::write(
            &p,
            r#"
clusters: [{name: prod, cluster: {server: "https://a"}}]
users:
  - name: eks-u
    user:
      exec:
        apiVersion: client.authentication.k8s.io/v1beta1
        command: aws
        provideClusterInfo: true
contexts: [{name: prod, context: {cluster: prod, user: eks-u}}]
"#,
        )
        .unwrap();
        let mut store = KubeconfigStore::load(vec![p.clone()]).unwrap();
        let spec: ContextSpec = serde_json::from_str(
            r#"{"name":"prod","originalName":"prod",
                "cluster":{"existing":"prod"},
                "user":{"name":"eks-u","execCommand":"aws","execArgs":["eks","get-token"]}}"#,
        )
        .unwrap();
        apply_context_spec(&mut store, spec).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(
            text.contains("provideClusterInfo"),
            "exec extras lost: {text}"
        );
    }
}
