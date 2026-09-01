use crate::resources::api_resource;
use kube::api::{Api, DeleteParams, DynamicObject, Patch, PatchParams, PropagationPolicy};
use kube::Client;
use serde_json::{json, Map, Value};

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

/// RFC 7386 JSON Merge Patch turning `base` into `edited`: keys dropped by the
/// edit become `null`, objects are diffed recursively, and everything else
/// (arrays included) is replaced wholesale.
pub fn merge_patch_diff(base: &Value, edited: &Value) -> Value {
    let (Value::Object(b), Value::Object(e)) = (base, edited) else {
        return edited.clone();
    };
    let mut out = Map::new();
    for (k, bv) in b {
        match e.get(k) {
            None => {
                out.insert(k.clone(), Value::Null);
            }
            Some(ev) if bv != ev => {
                let sub = merge_patch_diff(bv, ev);
                let vacuous = sub.as_object().is_some_and(Map::is_empty) && bv.is_object();
                if !vacuous {
                    out.insert(k.clone(), sub);
                }
            }
            Some(_) => {}
        }
    }
    for (k, ev) in e {
        if !b.contains_key(k) {
            out.insert(k.clone(), ev.clone());
        }
    }
    Value::Object(out)
}

/// Leaf paths a merge patch touches; descent stops at anything but a non-empty
/// object, so a replaced array or a deletion yields the path of the key itself.
pub fn patch_paths(patch: &Value) -> Vec<Vec<String>> {
    fn walk(v: &Value, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        match v {
            Value::Object(m) if !m.is_empty() => {
                for (k, sub) in m {
                    prefix.push(k.clone());
                    walk(sub, prefix, out);
                    prefix.pop();
                }
            }
            _ => out.push(prefix.clone()),
        }
    }
    let mut out = Vec::new();
    if patch.as_object().is_some_and(Map::is_empty) {
        return out;
    }
    walk(patch, &mut Vec::new(), &mut out);
    out
}

fn value_at<'a>(v: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut cur = v;
    for k in path {
        cur = cur.as_object()?.get(k)?;
    }
    Some(cur)
}

/// Drop from a patch the fields the server owns; kxs must never send them.
fn strip_server_fields(patch: &mut Value) {
    let Some(obj) = patch.as_object_mut() else {
        return;
    };
    obj.remove("status");
    let md_empty = match obj.get_mut("metadata").and_then(Value::as_object_mut) {
        Some(md) => {
            for k in [
                "resourceVersion",
                "uid",
                "generation",
                "creationTimestamp",
                "managedFields",
            ] {
                md.remove(k);
            }
            md.is_empty()
        }
        None => false,
    };
    if md_empty {
        obj.remove("metadata");
    }
}

fn identity_of(
    v: &Value,
) -> (
    Option<&Value>,
    Option<&Value>,
    Option<&Value>,
    Option<&Value>,
) {
    (
        v.get("apiVersion"),
        v.get("kind"),
        value_at(v, &["metadata".into(), "name".into()]),
        value_at(v, &["metadata".into(), "namespace".into()]),
    )
}

/// Apply an edit the way `kubectl edit` does: diff the user's edit against the
/// document they opened and send only that as a merge patch, so kxs neither
/// resends `resourceVersion` (which SSA treats as a precondition and controllers
/// invalidate constantly) nor steals field ownership from Helm/ArgoCD.
///
/// Returns the fresh server YAML after a real apply, or `None` when nothing was
/// written (dry run, or an edit that changed nothing).
#[allow(clippy::too_many_arguments)]
pub async fn apply_edit(
    client: Client,
    group: &str,
    version: &str,
    kind: &str,
    plural: &str,
    namespace: Option<&str>,
    name: &str,
    base_yaml: &str,
    edited_yaml: &str,
    dry_run: bool,
) -> Result<Option<String>, String> {
    let base: Value = serde_yaml_ng::from_str(base_yaml).map_err(|e| e.to_string())?;
    let edited: Value = serde_yaml_ng::from_str(edited_yaml).map_err(|e| e.to_string())?;
    if identity_of(&base) != identity_of(&edited) {
        return Err("apiVersion/kind/name/namespace cannot be changed in the editor".into());
    }

    let mut patch = merge_patch_diff(&base, &edited);
    strip_server_fields(&mut patch);
    let paths = patch_paths(&patch);
    if paths.is_empty() {
        return Ok(None);
    }

    let ar = api_resource(group, version, kind, plural);
    let api = dyn_api(client.clone(), &ar, namespace);
    let latest = serde_json::to_value(api.get(name).await.map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let stale: Vec<String> = paths
        .iter()
        .filter(|p| value_at(&base, p) != value_at(&latest, p))
        .map(|p| p.join("."))
        .collect();
    if !stale.is_empty() {
        return Err(format!(
            "conflict: {} changed on the server since you opened the editor; reload and re-apply",
            stale.join(", ")
        ));
    }

    let pp = PatchParams {
        field_manager: Some("kxs".into()),
        dry_run,
        ..Default::default()
    };
    api.patch(name, &pp, &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;
    if dry_run {
        return Ok(None);
    }
    crate::resources::get_yaml(client, group, version, kind, plural, namespace, name)
        .await
        .map(Some)
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
    fn diff_of_identical_docs_is_empty() {
        let v = json!({"spec": {"replicas": 2}, "metadata": {"name": "web"}});
        assert_eq!(merge_patch_diff(&v, &v), json!({}));
        assert!(patch_paths(&merge_patch_diff(&v, &v)).is_empty());
    }

    #[test]
    fn diff_emits_only_changed_scalar() {
        let base = json!({"spec": {"replicas": 2, "paused": false}});
        let edited = json!({"spec": {"replicas": 5, "paused": false}});
        assert_eq!(
            merge_patch_diff(&base, &edited),
            json!({"spec": {"replicas": 5}})
        );
    }

    #[test]
    fn diff_nulls_removed_keys() {
        let base = json!({"metadata": {"labels": {"a": "1", "b": "2"}}});
        let edited = json!({"metadata": {"labels": {"a": "1"}}});
        assert_eq!(
            merge_patch_diff(&base, &edited),
            json!({"metadata": {"labels": {"b": null}}})
        );
    }

    #[test]
    fn diff_recurses_into_nested_objects_and_adds_keys() {
        let base = json!({"spec": {"template": {"spec": {"nodeName": "n1"}}}});
        let edited =
            json!({"spec": {"template": {"spec": {"nodeName": "n2"}}}, "spec2": {"x": true}});
        assert_eq!(
            merge_patch_diff(&base, &edited),
            json!({"spec": {"template": {"spec": {"nodeName": "n2"}}}, "spec2": {"x": true}})
        );
    }

    #[test]
    fn diff_replaces_whole_list() {
        let base = json!({"spec": {"ports": [{"port": 80}, {"port": 443}]}});
        let edited = json!({"spec": {"ports": [{"port": 8080}]}});
        assert_eq!(
            merge_patch_diff(&base, &edited),
            json!({"spec": {"ports": [{"port": 8080}]}})
        );
    }

    #[test]
    fn patch_paths_lists_touched_leaves() {
        let patch = json!({
            "spec": {"replicas": 3, "ports": [1, 2]},
            "metadata": {"labels": {"gone": null}},
        });
        let mut got = patch_paths(&patch);
        got.sort();
        assert_eq!(
            got,
            vec![
                vec!["metadata", "labels", "gone"],
                vec!["spec", "ports"],
                vec!["spec", "replicas"],
            ]
        );
        assert!(patch_paths(&json!({})).is_empty());
    }

    #[test]
    fn strip_server_fields_prunes_empty_metadata() {
        let mut p = json!({
            "status": {"replicas": 1},
            "metadata": {"resourceVersion": "1", "uid": "u", "generation": 2,
                         "creationTimestamp": "t", "managedFields": []},
            "spec": {"replicas": 3},
        });
        strip_server_fields(&mut p);
        assert_eq!(p, json!({"spec": {"replicas": 3}}));
    }

    #[tokio::test]
    async fn apply_edit_rejects_identity_changes() {
        let base = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: a\n  namespace: n\n";
        let edited = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: b\n  namespace: n\n";
        let err = apply_edit(
            Client::try_from(kube::Config::new("http://127.0.0.1:1".parse().unwrap())).unwrap(),
            "",
            "v1",
            "ConfigMap",
            "configmaps",
            Some("n"),
            "a",
            base,
            edited,
            true,
        )
        .await
        .unwrap_err();
        assert!(err.contains("cannot be changed"), "{err}");
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
        // own namespace: sharing one with other tests races their cleanup
        fresh_namespace(&s.client, "kxs-e2e-cm").await;

        let cm = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: kxs-t\n  namespace: kxs-e2e-cm\ndata:\n  a: \"1\"\n";
        // dry-run must not persist
        apply_yaml(
            s.client.clone(),
            "",
            "v1",
            "ConfigMap",
            "configmaps",
            Some("kxs-e2e-cm"),
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
            Some("kxs-e2e-cm"),
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
            Some("kxs-e2e-cm"),
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
            Some("kxs-e2e-cm"),
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
            Some("kxs-e2e-cm"),
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
            "kxs-e2e-cm",
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

    /// A clean namespace for an e2e test: residue from an aborted run, or a
    /// namespace still terminating, makes a plain create fail.
    async fn fresh_namespace(client: &Client, name: &str) {
        use k8s_openapi::api::core::v1::Namespace;
        let api: Api<Namespace> = Api::all(client.clone());
        let _ = api.delete(name, &DeleteParams::default()).await;
        for _ in 0..120 {
            match api.get_opt(name).await {
                Ok(None) => break,
                _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
            }
        }
        let ns: Namespace = serde_json::from_value(json!({"metadata": {"name": name}})).unwrap();
        api.create(&kube::api::PostParams::default(), &ns)
            .await
            .expect("create test namespace");
    }

    async fn edit_fixture(ns: &str) -> (crate::session::ClusterSession, String) {
        use k8s_openapi::api::core::v1::ConfigMap;
        let s = kind_session().await;
        fresh_namespace(&s.client, ns).await;
        let api: Api<ConfigMap> = Api::namespaced(s.client.clone(), ns);
        let cm: ConfigMap = serde_json::from_value(json!({
            "metadata": {"name": "kxs-edit", "namespace": ns},
            "data": {"a": "1", "b": "2"},
        }))
        .unwrap();
        api.create(&kube::api::PostParams::default(), &cm)
            .await
            .unwrap();
        let base = cm_yaml(&s.client, ns).await;
        (s, base)
    }

    async fn cm_yaml(client: &Client, ns: &str) -> String {
        crate::resources::get_yaml(
            client.clone(),
            "",
            "v1",
            "ConfigMap",
            "configmaps",
            Some(ns),
            "kxs-edit",
        )
        .await
        .unwrap()
    }

    async fn cm_object(client: &Client, ns: &str) -> DynamicObject {
        dyn_api(
            client.clone(),
            &api_resource("", "v1", "ConfigMap", "configmaps"),
            Some(ns),
        )
        .get("kxs-edit")
        .await
        .unwrap()
    }

    /// The editor's view of a `data` edit: set a key, or `None` to delete it.
    fn with_data(yaml: &str, key: &str, value: Option<&str>) -> String {
        let mut v: Value = serde_yaml_ng::from_str(yaml).unwrap();
        let data = v.get_mut("data").unwrap().as_object_mut().unwrap();
        match value {
            Some(s) => data.insert(key.into(), json!(s)),
            None => data.remove(key),
        };
        serde_yaml_ng::to_string(&v).unwrap()
    }

    async fn apply_edited(
        s: &crate::session::ClusterSession,
        ns: &str,
        base: &str,
        edited: &str,
    ) -> Result<Option<String>, String> {
        apply_edit(
            s.client.clone(),
            "",
            "v1",
            "ConfigMap",
            "configmaps",
            Some(ns),
            "kxs-edit",
            base,
            edited,
            false,
        )
        .await
    }

    async fn drop_namespace(client: &Client, ns: &str) {
        let _ = delete_resource(
            client.clone(),
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

    /// The 409 regression: an out-of-band write bumps resourceVersion without
    /// touching what the user edited. Full-object SSA rejects the now-stale
    /// manifest; the merge-patch diff applies cleanly.
    #[tokio::test]
    #[ignore]
    async fn edit_survives_out_of_band_resource_version_bump() {
        const NS: &str = "kxs-e2e-edit409-rv";
        let (s, base) = edit_fixture(NS).await;

        merge_patch(
            s.client.clone(),
            "",
            "v1",
            "ConfigMap",
            "configmaps",
            Some(NS),
            "kxs-edit",
            json!({"metadata": {"annotations": {"kxs.test/oob": "1"}}}),
        )
        .await
        .unwrap();

        let stale_ssa = apply_yaml(
            s.client.clone(),
            "",
            "v1",
            "ConfigMap",
            "configmaps",
            Some(NS),
            "kxs-edit",
            &base,
            false,
        )
        .await;
        assert!(
            stale_ssa.is_err(),
            "full-object SSA is expected to reject the stale resourceVersion"
        );

        let fresh = apply_edited(&s, NS, &base, &with_data(&base, "a", Some("9")))
            .await
            .unwrap()
            .expect("a real apply returns fresh YAML");
        let got: Value = serde_yaml_ng::from_str(&fresh).unwrap();
        assert_eq!(got["data"]["a"], json!("9"));
        assert_eq!(got["data"]["b"], json!("2"));
        assert_eq!(
            got["metadata"]["annotations"]["kxs.test/oob"],
            json!("1"),
            "the out-of-band write must survive the edit"
        );

        drop_namespace(&s.client, NS).await;
    }

    #[tokio::test]
    #[ignore]
    async fn concurrent_change_to_edited_field_is_a_conflict() {
        const NS: &str = "kxs-e2e-edit409-conflict";
        let (s, base) = edit_fixture(NS).await;

        merge_patch(
            s.client.clone(),
            "",
            "v1",
            "ConfigMap",
            "configmaps",
            Some(NS),
            "kxs-edit",
            json!({"data": {"a": "someone-else"}}),
        )
        .await
        .unwrap();

        let err = apply_edited(&s, NS, &base, &with_data(&base, "a", Some("9")))
            .await
            .unwrap_err();
        assert!(err.starts_with("conflict: data.a "), "{err}");
        let latest = cm_object(&s.client, NS).await;
        assert_eq!(latest.data["data"]["a"], json!("someone-else"));

        drop_namespace(&s.client, NS).await;
    }

    #[tokio::test]
    #[ignore]
    async fn deleting_a_field_in_the_editor_deletes_it_on_the_server() {
        const NS: &str = "kxs-e2e-edit409-delete";
        let (s, base) = edit_fixture(NS).await;

        apply_edited(&s, NS, &base, &with_data(&base, "b", None))
            .await
            .unwrap()
            .unwrap();
        let latest = cm_object(&s.client, NS).await;
        assert_eq!(latest.data["data"]["a"], json!("1"));
        assert!(
            latest.data["data"].get("b").is_none(),
            "data.b should be gone: {}",
            latest.data["data"]
        );

        drop_namespace(&s.client, NS).await;
    }

    #[tokio::test]
    #[ignore]
    async fn no_op_edit_does_not_write() {
        const NS: &str = "kxs-e2e-edit409-noop";
        let (s, base) = edit_fixture(NS).await;

        let before = cm_object(&s.client, NS).await.metadata.resource_version;
        assert_eq!(apply_edited(&s, NS, &base, &base).await.unwrap(), None);
        let after = cm_object(&s.client, NS).await.metadata.resource_version;
        assert_eq!(before, after, "a no-op edit must not write");

        drop_namespace(&s.client, NS).await;
    }

    #[tokio::test]
    #[ignore]
    async fn apply_claims_only_the_edited_fields() {
        const NS: &str = "kxs-e2e-edit409-managed";
        let (s, base) = edit_fixture(NS).await;

        apply_edited(&s, NS, &base, &with_data(&base, "a", Some("9")))
            .await
            .unwrap()
            .unwrap();

        let latest = cm_object(&s.client, NS).await;
        let entry = latest
            .metadata
            .managed_fields
            .unwrap_or_default()
            .into_iter()
            .find(|e| e.manager.as_deref() == Some("kxs"))
            .expect("kxs managed-fields entry");
        assert_eq!(entry.operation.as_deref(), Some("Update"));
        let fields = serde_json::to_string(&entry.fields_v1).unwrap();
        assert!(fields.contains("f:a"), "{fields}");
        assert!(!fields.contains("f:b"), "{fields}");

        drop_namespace(&s.client, NS).await;
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
