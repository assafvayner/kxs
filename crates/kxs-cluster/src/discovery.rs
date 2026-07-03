use kube::discovery::{Discovery, Scope};
use kube::Client;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceKind {
    pub group: String,
    pub version: String,
    pub kind: String,
    pub plural: String,
    pub namespaced: bool,
    pub aliases: Vec<String>,
}

/// kubectl/k9s short names for common builtins, keyed by (group, kind).
fn short_names(group: &str, kind: &str) -> &'static [&'static str] {
    match (group, kind) {
        ("", "Pod") => &["po"],
        ("", "Service") => &["svc"],
        ("", "Namespace") => &["ns"],
        ("", "Node") => &["no"],
        ("", "ConfigMap") => &["cm"],
        ("", "Secret") => &[],
        ("", "PersistentVolume") => &["pv"],
        ("", "PersistentVolumeClaim") => &["pvc"],
        ("", "ServiceAccount") => &["sa"],
        ("", "Endpoints") => &["ep"],
        ("", "Event") => &["ev"],
        ("", "ReplicationController") => &["rc"],
        ("", "LimitRange") => &["limits"],
        ("", "ResourceQuota") => &["quota"],
        ("apps", "Deployment") => &["deploy"],
        ("apps", "ReplicaSet") => &["rs"],
        ("apps", "StatefulSet") => &["sts"],
        ("apps", "DaemonSet") => &["ds"],
        ("batch", "Job") => &[],
        ("batch", "CronJob") => &["cj"],
        ("networking.k8s.io", "Ingress") => &["ing"],
        ("networking.k8s.io", "NetworkPolicy") => &["netpol"],
        ("rbac.authorization.k8s.io", "ClusterRole") => &[],
        ("rbac.authorization.k8s.io", "ClusterRoleBinding") => &[],
        ("rbac.authorization.k8s.io", "Role") => &[],
        ("rbac.authorization.k8s.io", "RoleBinding") => &[],
        ("storage.k8s.io", "StorageClass") => &["sc"],
        ("policy", "PodDisruptionBudget") => &["pdb"],
        ("autoscaling", "HorizontalPodAutoscaler") => &["hpa"],
        ("apiextensions.k8s.io", "CustomResourceDefinition") => &["crd", "crds"],
        _ => &[],
    }
}

/// Command-bar aliases: lowercased kind, plural, and any short names. Pre-sorted/deduped.
pub fn alias_set(kind: &str, plural: &str, group: &str) -> Vec<String> {
    let mut out = vec![kind.to_lowercase(), plural.to_lowercase()];
    out.extend(short_names(group, kind).iter().map(|s| s.to_string()));
    out.sort();
    out.dedup();
    out
}

pub async fn discover(client: Client) -> Result<Vec<ResourceKind>, String> {
    let disc = Discovery::new(client)
        .run()
        .await
        .map_err(|e| e.to_string())?;
    let mut out: Vec<ResourceKind> = Vec::new();
    for group in disc.groups() {
        for (ar, caps) in group.recommended_resources() {
            if out.iter().any(|k| k.group == ar.group && k.kind == ar.kind) {
                continue;
            }
            out.push(ResourceKind {
                aliases: alias_set(&ar.kind, &ar.plural, &ar.group),
                group: ar.group.clone(),
                version: ar.version.clone(),
                kind: ar.kind.clone(),
                plural: ar.plural.clone(),
                namespaced: matches!(caps.scope, Scope::Namespaced),
            });
        }
    }
    out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.group.cmp(&b.group)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_short_names() {
        let a = alias_set("Pod", "pods", "");
        assert!(a.contains(&"po".to_string()));
        assert!(a.contains(&"pod".to_string()));
        assert!(a.contains(&"pods".to_string()));
        let s = alias_set("Service", "services", "");
        assert!(s.contains(&"svc".to_string()));
        let d = alias_set("Deployment", "deployments", "apps");
        assert!(d.contains(&"deploy".to_string()));
        assert!(d.contains(&"deployment".to_string()));
    }

    #[test]
    fn crd_kind_gets_derived_aliases() {
        let a = alias_set("FooBar", "foobars", "example.com");
        assert!(a.contains(&"foobar".to_string()));
        assert!(a.contains(&"foobars".to_string()));
        // no fabricated short name for unknown kinds
        assert_eq!(a.iter().filter(|x| x.len() < 4).count(), 0);
    }

    #[test]
    fn aliases_are_lowercased_and_deduped() {
        let a = alias_set("Pod", "pods", "");
        let mut sorted = a.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(a.len(), sorted.len(), "aliases must be pre-deduped: {a:?}");
        assert!(a.iter().all(|x| x == &x.to_lowercase()));
    }

    #[tokio::test]
    #[ignore]
    async fn discovers_builtins_on_kind_local() {
        let paths = kxs_core::kubeconfig::paths::kubeconfig_paths();
        let store = kxs_core::kubeconfig::store::KubeconfigStore::load(paths).unwrap();
        let yaml = crate::bridge::kubeconfig_yaml_for_context(&store, "kind-local").unwrap();
        let session = crate::session::connect(&yaml, "kind-local").await.unwrap();
        let kinds = discover(session.client.clone()).await.unwrap();
        assert!(kinds.iter().any(|k| k.kind == "Pod" && k.namespaced));
        assert!(kinds.iter().any(|k| k.kind == "Node" && !k.namespaced));
        assert!(kinds
            .iter()
            .any(|k| k.kind == "Deployment" && k.group == "apps"));
    }
}
