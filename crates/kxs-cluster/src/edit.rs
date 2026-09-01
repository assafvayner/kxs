use crate::resources::api_resource;
use kube::api::{Api, DeleteParams, DynamicObject, Patch, PatchParams, PropagationPolicy};
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

pub fn suspend_patch(suspend: bool) -> serde_json::Value {
    json!({ "spec": { "suspend": suspend } })
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

/// DeleteParams for the given propagation policy name and force flag.
/// `None`/empty propagation leaves the server's per-resource default in place;
/// force means grace period 0 (kubectl's `--force` for stuck objects).
pub fn delete_params(propagation: Option<&str>, force: bool) -> Result<DeleteParams, String> {
    let mut dp = DeleteParams::default();
    match propagation.map(str::trim) {
        None | Some("") => {}
        Some("Background") => dp.propagation_policy = Some(PropagationPolicy::Background),
        Some("Foreground") => dp.propagation_policy = Some(PropagationPolicy::Foreground),
        Some("Orphan") => dp.propagation_policy = Some(PropagationPolicy::Orphan),
        Some(other) => return Err(format!("unknown propagation policy: {other}")),
    }
    if force {
        dp.grace_period_seconds = Some(0);
    }
    Ok(dp)
}

#[allow(clippy::too_many_arguments)]
pub async fn delete_resource(
    client: Client,
    group: &str,
    version: &str,
    kind: &str,
    plural: &str,
    namespace: Option<&str>,
    name: &str,
    propagation: Option<&str>,
    force: bool,
) -> Result<(), String> {
    let dp = delete_params(propagation, force)?;
    let ar = api_resource(group, version, kind, plural);
    let api = dyn_api(client, &ar, namespace);
    api.delete(name, &dp).await.map_err(|e| e.to_string())?;
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
    fn suspend_patch_shape() {
        assert_eq!(
            suspend_patch(true),
            serde_json::json!({"spec": {"suspend": true}})
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

    #[test]
    fn delete_params_default_is_untouched() {
        assert_eq!(delete_params(None, false).unwrap(), DeleteParams::default());
        assert_eq!(
            delete_params(Some(""), false).unwrap(),
            DeleteParams::default()
        );
    }

    #[test]
    fn delete_params_maps_propagation() {
        for (name, want) in [
            ("Background", PropagationPolicy::Background),
            ("Foreground", PropagationPolicy::Foreground),
            ("Orphan", PropagationPolicy::Orphan),
        ] {
            let dp = delete_params(Some(name), false).unwrap();
            assert_eq!(dp.propagation_policy, Some(want));
            assert_eq!(dp.grace_period_seconds, None);
        }
    }

    #[test]
    fn delete_params_force_sets_zero_grace_period() {
        let dp = delete_params(Some("Foreground"), true).unwrap();
        assert_eq!(dp.grace_period_seconds, Some(0));
        assert_eq!(dp.propagation_policy, Some(PropagationPolicy::Foreground));
        assert_eq!(
            delete_params(None, true).unwrap().grace_period_seconds,
            Some(0)
        );
    }

    #[test]
    fn delete_params_rejects_unknown_propagation() {
        assert!(delete_params(Some("background"), false).is_err());
        assert!(delete_params(Some("nope"), false).is_err());
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
            None,
            false,
        )
        .await
        .unwrap();
        // cleanup the namespace too (best-effort; deletion is async)
        let _ = delete_resource(
            s.client.clone(),
            "",
            "v1",
            "Namespace",
            "namespaces",
            None,
            "kxs-e2e",
            None,
            false,
        )
        .await;
    }

    #[tokio::test]
    #[ignore]
    async fn orphan_delete_leaves_dependents_behind() {
        let s = kind_session().await;
        let ns = "kxs-e2e-del";
        let ns_yaml = format!("apiVersion: v1\nkind: Namespace\nmetadata:\n  name: {ns}\n");
        apply_yaml(
            s.client.clone(),
            "",
            "v1",
            "Namespace",
            "namespaces",
            None,
            ns,
            &ns_yaml,
            false,
        )
        .await
        .unwrap();

        let dep = format!("apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: kxs-orphan\n  namespace: {ns}\nspec:\n  replicas: 1\n  selector:\n    matchLabels: {{app: kxs-orphan}}\n  template:\n    metadata: {{labels: {{app: kxs-orphan}}}}\n    spec:\n      containers: [{{name: c, image: registry.k8s.io/pause:3.9}}]\n");
        apply_yaml(
            s.client.clone(),
            "apps",
            "v1",
            "Deployment",
            "deployments",
            Some(ns),
            "kxs-orphan",
            &dep,
            false,
        )
        .await
        .unwrap();

        // wait for the ReplicaSet the deployment owns to appear
        let rs_api: Api<DynamicObject> = dyn_api(
            s.client.clone(),
            &api_resource("apps", "v1", "ReplicaSet", "replicasets"),
            Some(ns),
        );
        let mut rs_name = None;
        for _ in 0..60 {
            let list = rs_api
                .list(&kube::api::ListParams::default())
                .await
                .unwrap();
            if let Some(rs) = list.items.into_iter().find(|r| {
                r.metadata
                    .name
                    .as_deref()
                    .is_some_and(|n| n.starts_with("kxs-orphan-"))
            }) {
                rs_name = rs.metadata.name.clone();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        let rs_name = rs_name.expect("deployment never produced a ReplicaSet");

        delete_resource(
            s.client.clone(),
            "apps",
            "v1",
            "Deployment",
            "deployments",
            Some(ns),
            "kxs-orphan",
            Some("Orphan"),
            false,
        )
        .await
        .unwrap();

        // The orphaned ReplicaSet outlives its owner. Orphaning is async: the
        // delete call returns once the orphan finalizer is set, and the GC
        // strips ownerReferences afterwards — so poll instead of asserting
        // immediately.
        let mut orphaned = false;
        for _ in 0..30 {
            let rs = rs_api.get(&rs_name).await.unwrap();
            if rs.metadata.owner_references.unwrap_or_default().is_empty() {
                orphaned = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        assert!(
            orphaned,
            "ownerReferences never removed from orphaned ReplicaSet"
        );

        // cleanup: namespace deletion reaps the orphan
        let _ = delete_resource(
            s.client.clone(),
            "",
            "v1",
            "Namespace",
            "namespaces",
            None,
            ns,
            None,
            false,
        )
        .await;
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
