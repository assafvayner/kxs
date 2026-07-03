use crate::resources::api_resource;
use kube::api::{Api, DeleteParams, DynamicObject, Patch, PatchParams};
use kube::Client;
use serde_json::json;

pub struct ParsedManifest {
    pub group: String,
    pub version: String,
    pub kind: String,
    pub plural: String,
    pub namespace: Option<String>,
    pub name: String,
    pub object: DynamicObject,
}

/// Parse edited YAML, deriving GVK+name+namespace from apiVersion/kind/metadata.
/// Plural is guessed via ApiResource::from_gvk (SSA tolerates the guess for
/// built-ins; callers with a discovered plural should prefer `apply_yaml`'s explicit args).
pub fn parse_manifest(yaml: &str) -> Result<ParsedManifest, String> {
    let object: DynamicObject = serde_yaml_ng::from_str(yaml).map_err(|e| e.to_string())?;
    let types = object
        .types
        .clone()
        .ok_or("manifest missing apiVersion/kind")?;
    let (group, version) = match types.api_version.split_once('/') {
        Some((g, v)) => (g.to_string(), v.to_string()),
        None => (String::new(), types.api_version.clone()),
    };
    if types.kind.is_empty() {
        return Err("manifest missing kind".into());
    }
    let name = object
        .metadata
        .name
        .clone()
        .ok_or("manifest missing metadata.name")?;
    let namespace = object.metadata.namespace.clone();
    let ar = kube::core::ApiResource::from_gvk(&kube::core::GroupVersionKind {
        group: group.clone(),
        version: version.clone(),
        kind: types.kind.clone(),
    });
    Ok(ParsedManifest {
        group,
        version,
        kind: types.kind,
        plural: ar.plural,
        namespace,
        name,
        object,
    })
}

pub fn scale_patch(replicas: i32) -> serde_json::Value {
    json!({ "spec": { "replicas": replicas } })
}

pub fn restart_patch(now_rfc3339: &str) -> serde_json::Value {
    json!({ "spec": { "template": { "metadata": { "annotations": {
        "kubectl.kubernetes.io/restartedAt": now_rfc3339
    }}}}})
}

pub fn cordon_patch(unschedulable: bool) -> serde_json::Value {
    json!({ "spec": { "unschedulable": unschedulable } })
}

fn dyn_api(
    client: Client,
    ar: &kube::core::ApiResource,
    namespace: Option<&str>,
) -> Api<DynamicObject> {
    match namespace {
        Some(ns) if !ns.is_empty() => Api::namespaced_with(client, ns, ar),
        _ => Api::all_with(client, ar),
    }
}

/// Server-side apply the edited manifest. `dry_run` validates without persisting.
/// Uses the caller-supplied (discovered) plural so CRDs work.
#[allow(clippy::too_many_arguments)]
pub async fn apply_yaml(
    client: Client,
    group: &str,
    version: &str,
    kind: &str,
    plural: &str,
    namespace: Option<&str>,
    name: &str,
    yaml: &str,
    dry_run: bool,
) -> Result<(), String> {
    let object: DynamicObject = serde_yaml_ng::from_str(yaml).map_err(|e| e.to_string())?;
    let ar = api_resource(group, version, kind, plural);
    let api = dyn_api(client, &ar, namespace);
    let mut pp = PatchParams::apply("kxs").force();
    if dry_run {
        pp = pp.dry_run();
    }
    api.patch(name, &pp, &Patch::Apply(&object))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn delete_resource(
    client: Client,
    group: &str,
    version: &str,
    kind: &str,
    plural: &str,
    namespace: Option<&str>,
    name: &str,
) -> Result<(), String> {
    let ar = api_resource(group, version, kind, plural);
    let api = dyn_api(client, &ar, namespace);
    api.delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Merge-patch helper for scale/restart/cordon.
#[allow(clippy::too_many_arguments)]
pub async fn merge_patch(
    client: Client,
    group: &str,
    version: &str,
    kind: &str,
    plural: &str,
    namespace: Option<&str>,
    name: &str,
    patch: serde_json::Value,
) -> Result<(), String> {
    let ar = api_resource(group, version, kind, plural);
    let api = dyn_api(client, &ar, namespace);
    api.patch(name, &PatchParams::default(), &Patch::Merge(patch))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gvk_from_manifest() {
        let y = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\n  namespace: app\n";
        let m = parse_manifest(y).unwrap();
        assert_eq!(m.group, "apps");
        assert_eq!(m.version, "v1");
        assert_eq!(m.kind, "Deployment");
        assert_eq!(m.namespace.as_deref(), Some("app"));
        assert_eq!(m.name, "web");
    }

    #[test]
    fn parses_core_group() {
        let y = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: cfg\n  namespace: app\n";
        let m = parse_manifest(y).unwrap();
        assert_eq!(m.group, "");
        assert_eq!(m.version, "v1");
        assert_eq!(m.kind, "ConfigMap");
    }

    #[test]
    fn manifest_missing_kind_errors() {
        assert!(parse_manifest("metadata:\n  name: x\n").is_err());
    }

    #[test]
    fn manifest_missing_name_errors() {
        assert!(parse_manifest("apiVersion: v1\nkind: Pod\nmetadata: {}\n").is_err());
    }

    #[test]
    fn invalid_yaml_errors() {
        assert!(parse_manifest("apiVersion: v1\nkind: [not a string").is_err());
    }

    #[test]
    fn scale_patch_shape() {
        let p = scale_patch(3);
        assert_eq!(p, serde_json::json!({"spec": {"replicas": 3}}));
    }

    #[test]
    fn restart_patch_has_annotation() {
        let p = restart_patch("2026-07-03T00:00:00Z");
        let ann = &p["spec"]["template"]["metadata"]["annotations"];
        assert_eq!(
            ann["kubectl.kubernetes.io/restartedAt"],
            "2026-07-03T00:00:00Z"
        );
    }

    #[test]
    fn cordon_patch_shape() {
        assert_eq!(
            cordon_patch(true),
            serde_json::json!({"spec": {"unschedulable": true}})
        );
        assert_eq!(
            cordon_patch(false),
            serde_json::json!({"spec": {"unschedulable": false}})
        );
    }

    async fn kind_session() -> crate::session::ClusterSession {
        let paths = kxs_core::kubeconfig::paths::kubeconfig_paths();
        let store = kxs_core::kubeconfig::store::KubeconfigStore::load(paths).unwrap();
        let yaml = crate::bridge::kubeconfig_yaml_for_context(&store, "kind-local").unwrap();
        crate::session::connect(&yaml, "kind-local").await.unwrap()
    }

    #[tokio::test]
    #[ignore]
    async fn apply_dryrun_then_apply_then_delete_configmap() {
        let s = kind_session().await;
        // ensure namespace exists (best-effort apply)
        let ns_yaml = "apiVersion: v1\nkind: Namespace\nmetadata:\n  name: kxs-e2e\n";
        let _ = apply_yaml(
            s.client.clone(),
            "",
            "v1",
            "Namespace",
            "namespaces",
            None,
            "kxs-e2e",
            ns_yaml,
            false,
        )
        .await;

        let cm = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: kxs-t\n  namespace: kxs-e2e\ndata:\n  a: \"1\"\n";
        // dry-run must not persist
        apply_yaml(
            s.client.clone(),
            "",
            "v1",
            "ConfigMap",
            "configmaps",
            Some("kxs-e2e"),
            "kxs-t",
            cm,
            true,
        )
        .await
        .unwrap();
        let got = crate::resources::get_yaml(
            s.client.clone(),
            "",
            "v1",
            "ConfigMap",
            "configmaps",
            Some("kxs-e2e"),
            "kxs-t",
        )
        .await;
        assert!(got.is_err(), "dry-run must not create the object");
        // real apply
        apply_yaml(
            s.client.clone(),
            "",
            "v1",
            "ConfigMap",
            "configmaps",
            Some("kxs-e2e"),
            "kxs-t",
            cm,
            false,
        )
        .await
        .unwrap();
        let got = crate::resources::get_yaml(
            s.client.clone(),
            "",
            "v1",
            "ConfigMap",
            "configmaps",
            Some("kxs-e2e"),
            "kxs-t",
        )
        .await
        .unwrap();
        assert!(got.contains("kxs-t"));
        // cleanup
        delete_resource(
            s.client.clone(),
            "",
            "v1",
            "ConfigMap",
            "configmaps",
            Some("kxs-e2e"),
            "kxs-t",
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn invalid_apply_surfaces_server_error() {
        let s = kind_session().await;
        // negative replicas is rejected server-side even on dry-run
        let bad = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: kxs-bad\n  namespace: kxs-e2e\nspec:\n  replicas: -1\n  selector:\n    matchLabels: {app: x}\n  template:\n    metadata: {labels: {app: x}}\n    spec:\n      containers: [{name: c, image: nginx}]\n";
        let r = apply_yaml(
            s.client.clone(),
            "apps",
            "v1",
            "Deployment",
            "deployments",
            Some("kxs-e2e"),
            "kxs-bad",
            bad,
            true,
        )
        .await;
        assert!(r.is_err());
    }
}
