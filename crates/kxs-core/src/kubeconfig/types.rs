use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use std::collections::BTreeMap;

/// Unknown fields captured on every struct so write-back never drops data.
/// Intentional round-trip deviations (kubectl-equivalent, accepted by spec):
/// keys are re-ordered on write, and explicitly-empty lists (`contexts: []`)
/// are omitted rather than kept empty.
pub type Extras = BTreeMap<String, Value>;

fn default_api_version() -> String {
    "v1".into()
}
fn default_kind() -> String {
    "Config".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kubeconfig {
    #[serde(rename = "apiVersion", default = "default_api_version")]
    pub api_version: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clusters: Vec<NamedCluster>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub users: Vec<NamedAuthInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<NamedContext>,
    #[serde(
        rename = "current-context",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub current_context: Option<String>,
    #[serde(flatten)]
    pub extras: Extras,
}

impl Default for Kubeconfig {
    fn default() -> Self {
        Self {
            api_version: default_api_version(),
            kind: default_kind(),
            clusters: Vec::new(),
            users: Vec::new(),
            contexts: Vec::new(),
            current_context: None,
            extras: Extras::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedCluster {
    pub name: String,
    pub cluster: Cluster,
    #[serde(flatten)]
    pub extras: Extras,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cluster {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(
        rename = "certificate-authority",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub certificate_authority: Option<String>,
    #[serde(
        rename = "certificate-authority-data",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub certificate_authority_data: Option<String>,
    #[serde(
        rename = "insecure-skip-tls-verify",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub insecure_skip_tls_verify: Option<bool>,
    #[serde(flatten)]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedAuthInfo {
    pub name: String,
    pub user: AuthInfo,
    #[serde(flatten)]
    pub extras: Extras,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AuthInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(
        rename = "client-certificate",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub client_certificate: Option<String>,
    #[serde(
        rename = "client-certificate-data",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub client_certificate_data: Option<String>,
    #[serde(
        rename = "client-key",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub client_key: Option<String>,
    #[serde(
        rename = "client-key-data",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub client_key_data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec: Option<ExecConfig>,
    #[serde(flatten)]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecConfig {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<ExecEnv>>,
    #[serde(flatten)]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecEnv {
    pub name: String,
    pub value: String,
    #[serde(flatten)]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedContext {
    pub name: String,
    pub context: Context,
    #[serde(flatten)]
    pub extras: Extras,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub cluster: String,
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(flatten)]
    pub extras: Extras,
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const FIXTURE: &str = r#"
apiVersion: v1
kind: Config
current-context: prod
preferences:
  colors: true
clusters:
  - name: prod
    cluster:
      server: https://prod.example.com
      certificate-authority-data: LS0tCg==
      my-custom-field: keep-me
  - name: kind-local
    cluster:
      server: https://127.0.0.1:6443
      insecure-skip-tls-verify: true
users:
  - name: prod-user
    user:
      exec:
        apiVersion: client.authentication.k8s.io/v1beta1
        command: aws
        args: ["eks", "get-token", "--cluster-name", "prod"]
        env:
          - name: AWS_PROFILE
            value: prod
            custom-env-field: keep-me-too
  - name: kind-user
    user:
      token: abc123
contexts:
  - name: prod
    context:
      cluster: prod
      user: prod-user
      namespace: scanner
  - name: kind-local
    context:
      cluster: kind-local
      user: kind-user
"#;

    #[test]
    fn parses_fixture() {
        let cfg: Kubeconfig = serde_yaml_ng::from_str(FIXTURE).unwrap();
        assert_eq!(cfg.current_context.as_deref(), Some("prod"));
        assert_eq!(
            cfg.clusters[0].cluster.server.as_deref(),
            Some("https://prod.example.com")
        );
        assert_eq!(cfg.clusters[1].cluster.insecure_skip_tls_verify, Some(true));
        let exec = cfg.users[0].user.exec.as_ref().unwrap();
        assert_eq!(exec.command, "aws");
        assert_eq!(exec.args[1], "get-token");
        assert_eq!(exec.env.as_ref().unwrap()[0].name, "AWS_PROFILE");
        assert_eq!(cfg.users[1].user.token.as_deref(), Some("abc123"));
        assert_eq!(
            cfg.contexts[0].context.namespace.as_deref(),
            Some("scanner")
        );
    }

    #[test]
    fn round_trip_preserves_unknown_fields() {
        let cfg: Kubeconfig = serde_yaml_ng::from_str(FIXTURE).unwrap();
        let out = serde_yaml_ng::to_string(&cfg).unwrap();
        assert!(out.contains("preferences"), "top-level extras lost:\n{out}");
        assert!(
            out.contains("my-custom-field"),
            "nested extras lost:\n{out}"
        );
        assert!(
            out.contains("custom-env-field"),
            "exec env extras lost:\n{out}"
        );
        let reparsed: Kubeconfig = serde_yaml_ng::from_str(&out).unwrap();
        assert_eq!(cfg, reparsed);
    }

    #[test]
    fn tolerates_missing_api_version_and_kind() {
        let cfg: Kubeconfig = serde_yaml_ng::from_str("clusters: []").unwrap();
        assert_eq!(cfg.api_version, "v1");
        assert_eq!(cfg.kind, "Config");
        assert!(cfg.contexts.is_empty());
    }
}
