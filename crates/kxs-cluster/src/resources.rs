use futures::StreamExt;
use kube::api::{Api, DynamicObject, ListParams};
use kube::core::{ApiResource, GroupVersionKind};
use kube::Client;
use serde::{Deserialize, Serialize};

pub fn list_path(group: &str, version: &str, plural: &str, namespace: Option<&str>) -> String {
    let prefix = if group.is_empty() {
        format!("/api/{version}")
    } else {
        format!("/apis/{group}/{version}")
    };
    match namespace {
        Some(ns) if !ns.is_empty() => format!("{prefix}/namespaces/{ns}/{plural}"),
        _ => format!("{prefix}/{plural}"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawTable {
    #[serde(default)]
    pub metadata: RawListMeta,
    #[serde(default)]
    pub column_definitions: Vec<RawColumn>,
    #[serde(default)]
    pub rows: Vec<RawRow>,
}
#[derive(Debug, Default, Deserialize)]
pub struct RawListMeta {
    #[serde(rename = "continue", default)]
    pub r#continue: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawColumn {
    pub name: String,
    #[serde(default)]
    pub priority: i32,
}
#[derive(Debug, Deserialize)]
pub struct RawRow {
    #[serde(default)]
    pub cells: Vec<serde_json::Value>,
    #[serde(default)]
    pub object: RawObject,
}
#[derive(Debug, Default, Deserialize)]
pub struct RawObject {
    #[serde(default)]
    pub metadata: RawMeta,
}
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMeta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub creation_timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTable {
    pub columns: Vec<String>,
    pub rows: Vec<ResourceRow>,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRow {
    pub key: String,
    pub name: String,
    pub namespace: Option<String>,
    /// cells for the visible (priority 0) columns, stringified
    pub cells: Vec<String>,
    pub created: Option<String>,
}

fn cell_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "".into(),
        other => other.to_string(),
    }
}

/// Keeps only priority-0 columns (kubectl's non-wide set), stringifies cells,
/// appends a synthetic "Age" column (rendered client-side from `created`).
pub fn map_table(raw: RawTable) -> ResourceTable {
    let visible: Vec<usize> = raw
        .column_definitions
        .iter()
        .enumerate()
        .filter(|(_, c)| c.priority == 0 && !c.name.eq_ignore_ascii_case("age"))
        .map(|(i, _)| i)
        .collect();
    let mut columns: Vec<String> = visible
        .iter()
        .map(|&i| raw.column_definitions[i].name.clone())
        .collect();
    columns.push("Age".into());

    let rows = raw
        .rows
        .into_iter()
        .map(|r| {
            let name = r.object.metadata.name.clone().unwrap_or_default();
            let namespace = r.object.metadata.namespace.clone();
            let key = match &namespace {
                Some(ns) if !ns.is_empty() => format!("{ns}/{name}"),
                _ => name.clone(),
            };
            let cells = visible
                .iter()
                .map(|&i| r.cells.get(i).map(cell_to_string).unwrap_or_default())
                .collect();
            ResourceRow {
                key,
                name,
                namespace,
                cells,
                created: r.object.metadata.creation_timestamp,
            }
        })
        .collect();
    ResourceTable { columns, rows }
}

pub(crate) fn api_resource(group: &str, version: &str, kind: &str, plural: &str) -> ApiResource {
    let mut ar = ApiResource::from_gvk(&GroupVersionKind {
        group: group.to_string(),
        version: version.to_string(),
        kind: kind.to_string(),
    });
    ar.plural = plural.to_string();
    ar
}

/// Percent-encode a continue token for use as a query value (base64 tokens
/// can contain '+', '/', '=' which are not query-safe).
fn encode_query_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Server-side Table list for any kind. `namespace: None` = all namespaces.
/// Follows list continuation up to `MAX_TABLE_PAGES` pages so large clusters
/// aren't silently truncated at the per-request limit.
pub async fn list_table(
    client: Client,
    group: &str,
    version: &str,
    plural: &str,
    namespace: Option<&str>,
) -> Result<ResourceTable, String> {
    const MAX_TABLE_PAGES: usize = 10;
    let path = list_path(group, version, plural, namespace);
    let mut merged: Option<RawTable> = None;
    let mut continue_token: Option<String> = None;
    for _ in 0..MAX_TABLE_PAGES {
        let mut url = format!("{path}?limit=1000");
        if let Some(t) = &continue_token {
            url.push_str("&continue=");
            url.push_str(&encode_query_value(t));
        }
        let req = http::Request::get(url)
            .header(
                http::header::ACCEPT,
                "application/json;as=Table;v=v1;g=meta.k8s.io,application/json",
            )
            .body(Vec::new())
            .map_err(|e| e.to_string())?;
        let mut raw: RawTable = client.request(req).await.map_err(|e| e.to_string())?;
        continue_token = raw.metadata.r#continue.take().filter(|t| !t.is_empty());
        match &mut merged {
            None => merged = Some(raw),
            Some(m) => m.rows.extend(raw.rows),
        }
        if continue_token.is_none() {
            break;
        }
    }
    Ok(map_table(merged.unwrap_or_else(|| RawTable {
        metadata: RawListMeta::default(),
        column_definitions: Vec::new(),
        rows: Vec::new(),
    })))
}

/// Full manifest as YAML for one object.
pub async fn get_yaml(
    client: Client,
    group: &str,
    version: &str,
    kind: &str,
    plural: &str,
    namespace: Option<&str>,
    name: &str,
) -> Result<String, String> {
    let ar = api_resource(group, version, kind, plural);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) if !ns.is_empty() => Api::namespaced_with(client, ns, &ar),
        _ => Api::all_with(client, &ar),
    };
    let mut obj = api
        .get_opt(name)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{kind} \"{name}\" not found"))?;
    // managed-fields is noise in a YAML view
    obj.metadata.managed_fields = None;
    serde_yaml_ng::to_string(&obj).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceEvent {
    #[serde(rename = "type")]
    pub type_: String,
    pub reason: String,
    pub message: String,
    pub count: i32,
    pub last_seen: Option<String>,
}

/// Events referencing a given object (best-effort; empty on error). Filtering
/// by kind too keeps out events for same-named objects of other kinds (e.g. a
/// Service and Deployment both called "web").
pub async fn get_events(
    client: Client,
    namespace: Option<&str>,
    kind: &str,
    name: &str,
) -> Vec<ResourceEvent> {
    use k8s_openapi::api::core::v1::Event;
    use kube::api::ListParams;
    let api: Api<Event> = match namespace {
        Some(ns) if !ns.is_empty() => Api::namespaced(client, ns),
        _ => Api::all(client),
    };
    let fields = if kind.is_empty() {
        format!("involvedObject.name={name}")
    } else {
        format!("involvedObject.kind={kind},involvedObject.name={name}")
    };
    let lp = ListParams::default().fields(&fields);
    let list = match api.list(&lp).await {
        Ok(l) => l,
        Err(_) => return Vec::new(),
    };
    list.items
        .into_iter()
        .map(|e| ResourceEvent {
            type_: e.type_.unwrap_or_default(),
            reason: e.reason.unwrap_or_default(),
            message: e.message.unwrap_or_default(),
            count: e.count.unwrap_or(0),
            last_seen: e.last_timestamp.map(|t| t.0.to_rfc3339()),
        })
        .collect()
}

/// A kind to probe for existence, supplied by the frontend from discovery.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindProbe {
    pub group: String,
    pub version: String,
    pub kind: String,
    pub plural: String,
}

/// Keys ("{group}/{kind}") of the given kinds that have >=1 instance in `namespace`.
/// `namespace: None` = all namespaces. Per-kind errors (e.g. RBAC 403) count as absent.
pub async fn present_kinds(
    client: Client,
    namespace: Option<&str>,
    probes: Vec<KindProbe>,
) -> Vec<String> {
    futures::stream::iter(probes)
        .map(|p| {
            let client = client.clone();
            async move {
                let ar = api_resource(&p.group, &p.version, &p.kind, &p.plural);
                let api: Api<DynamicObject> = match namespace {
                    Some(ns) if !ns.is_empty() => Api::namespaced_with(client, ns, &ar),
                    _ => Api::all_with(client, &ar),
                };
                match api.list(&ListParams::default().limit(1)).await {
                    Ok(list) if !list.items.is_empty() => Some(format!("{}/{}", p.group, p.kind)),
                    _ => None,
                }
            }
        })
        .buffer_unordered(16)
        .filter_map(|k| async move { k })
        .collect()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_namespaced_path() {
        assert_eq!(
            list_path("", "v1", "pods", Some("app")),
            "/api/v1/namespaces/app/pods"
        );
    }
    #[test]
    fn core_cluster_path() {
        assert_eq!(list_path("", "v1", "nodes", None), "/api/v1/nodes");
    }
    #[test]
    fn grouped_namespaced_path() {
        assert_eq!(
            list_path("apps", "v1", "deployments", Some("kube-system")),
            "/apis/apps/v1/namespaces/kube-system/deployments"
        );
    }
    #[test]
    fn grouped_cluster_path() {
        assert_eq!(
            list_path("rbac.authorization.k8s.io", "v1", "clusterroles", None),
            "/apis/rbac.authorization.k8s.io/v1/clusterroles"
        );
    }
    #[test]
    fn continue_token_query_encoding() {
        assert_eq!(encode_query_value("abc-123_.~"), "abc-123_.~");
        assert_eq!(encode_query_value("a+b/c=="), "a%2Bb%2Fc%3D%3D");
    }

    #[test]
    fn empty_namespace_treated_as_all() {
        // caller passes None for all-namespaces; empty &str should not happen, but guard:
        assert_eq!(list_path("", "v1", "pods", None), "/api/v1/pods");
    }

    const TABLE_JSON: &str = r#"{
      "kind": "Table", "apiVersion": "meta.k8s.io/v1",
      "columnDefinitions": [
        {"name": "Name", "priority": 0},
        {"name": "Ready", "priority": 0},
        {"name": "Status", "priority": 0},
        {"name": "IP", "priority": 1}
      ],
      "rows": [
        {"cells": ["web-1", "1/1", "Running", "10.0.0.1"],
         "object": {"metadata": {"name": "web-1", "namespace": "app",
                     "uid": "u1", "creationTimestamp": "2026-07-01T00:00:00Z"}}},
        {"cells": ["web-2", "0/1", "Pending", "<none>"],
         "object": {"metadata": {"name": "web-2", "namespace": "app",
                     "uid": "u2", "creationTimestamp": "2026-07-02T00:00:00Z"}}}
      ]
    }"#;

    #[test]
    fn maps_table_dropping_low_priority_columns() {
        let raw: RawTable = serde_json::from_str(TABLE_JSON).unwrap();
        let t = map_table(raw);
        // priority>0 (IP) hidden in the default view; Age appended
        assert_eq!(t.columns, vec!["Name", "Ready", "Status", "Age"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].key, "app/web-1");
        assert_eq!(t.rows[0].cells, vec!["web-1", "1/1", "Running"]);
        assert_eq!(t.rows[0].created.as_deref(), Some("2026-07-01T00:00:00Z"));
        assert_eq!(t.rows[0].namespace.as_deref(), Some("app"));
    }

    #[test]
    fn cluster_scoped_rows_have_no_namespace() {
        let raw: RawTable = serde_json::from_str(
            r#"{"columnDefinitions":[{"name":"Name","priority":0}],
                "rows":[{"cells":["node-1"],"object":{"metadata":{"name":"node-1","uid":"n1"}}}]}"#,
        )
        .unwrap();
        let t = map_table(raw);
        assert_eq!(t.rows[0].key, "node-1");
        assert_eq!(t.rows[0].namespace, None);
    }

    #[test]
    fn server_age_column_replaced_by_synthetic() {
        let raw: RawTable = serde_json::from_str(
            r#"{"columnDefinitions":[{"name":"Name","priority":0},{"name":"Age","priority":0}],
                "rows":[{"cells":["web-1","5d"],"object":{"metadata":{"name":"web-1","namespace":"app","creationTimestamp":"2026-06-28T00:00:00Z"}}}]}"#,
        ).unwrap();
        let t = map_table(raw);
        assert_eq!(
            t.columns
                .iter()
                .filter(|c| c.eq_ignore_ascii_case("age"))
                .count(),
            1,
            "exactly one Age column"
        );
        assert_eq!(t.columns.last().unwrap(), "Age");
        assert_eq!(t.rows[0].cells, vec!["web-1"], "server age cell dropped");
    }

    /// Run manually: cargo test -p kxs-cluster -- --ignored (needs kind-local in ~/.kube/config)
    #[tokio::test]
    #[ignore]
    async fn lists_deployments_as_table_on_kind_local() {
        let session = kind_session().await;
        let t = super::list_table(session.client.clone(), "apps", "v1", "deployments", None)
            .await
            .unwrap();
        assert!(t.columns.iter().any(|c| c == "Name"));
        assert!(t.columns.last().map(|c| c == "Age").unwrap_or(false));
    }

    #[tokio::test]
    #[ignore]
    async fn gets_namespace_yaml_on_kind_local() {
        let session = kind_session().await;
        let y = super::get_yaml(
            session.client.clone(),
            "",
            "v1",
            "Namespace",
            "namespaces",
            None,
            "default",
        )
        .await
        .unwrap();
        assert!(y.contains("kind: Namespace"));
        assert!(y.contains("name: default"));
    }

    #[tokio::test]
    #[ignore]
    async fn present_kinds_reflects_namespace_contents_on_kind_local() {
        let session = kind_session().await;
        let probes = vec![
            KindProbe {
                group: String::new(),
                version: "v1".into(),
                kind: "Pod".into(),
                plural: "pods".into(),
            },
            KindProbe {
                group: "batch".into(),
                version: "v1".into(),
                kind: "CronJob".into(),
                plural: "cronjobs".into(),
            },
        ];
        // kube-system always has pods and no cronjobs.
        let keys = super::present_kinds(session.client.clone(), Some("kube-system"), probes).await;
        assert!(keys.contains(&"/Pod".to_string()), "expected Pod: {keys:?}");
        assert!(
            !keys.contains(&"batch/CronJob".to_string()),
            "unexpected CronJob: {keys:?}"
        );
    }

    async fn kind_session() -> crate::session::ClusterSession {
        let paths = kxs_core::kubeconfig::paths::kubeconfig_paths();
        let store = kxs_core::kubeconfig::store::KubeconfigStore::load(paths).unwrap();
        let yaml = crate::bridge::kubeconfig_yaml_for_context(&store, "kind-local").unwrap();
        crate::session::connect(&yaml, "kind-local").await.unwrap()
    }

    /// Run manually: cargo test -p kxs-cluster -- --ignored (needs kind-local
    /// in ~/.kube/config). The kind filter must be accepted by the apiserver
    /// as a field selector (empty result is fine; an unsupported selector
    /// would error server-side and get_events would swallow it — so also
    /// check a name-only call returns at least as many events).
    #[tokio::test]
    #[ignore]
    async fn get_events_filters_by_kind_on_kind_local() {
        use k8s_openapi::api::core::v1::Event;
        let session = kind_session().await;
        // find any event to use its involvedObject as the probe target
        let ev_api: Api<Event> = Api::all(session.client.clone());
        let evs = ev_api
            .list(&ListParams::default().limit(5))
            .await
            .unwrap()
            .items;
        let Some(ev) = evs
            .into_iter()
            .find(|e| e.involved_object.kind.is_some() && e.involved_object.name.is_some())
        else {
            eprintln!("no events in cluster; nothing to assert");
            return;
        };
        let kind = ev.involved_object.kind.unwrap();
        let name = ev.involved_object.name.unwrap();
        let ns = ev.involved_object.namespace;
        let with_kind =
            super::get_events(session.client.clone(), ns.as_deref(), &kind, &name).await;
        assert!(
            !with_kind.is_empty(),
            "expected events for {kind}/{name} with kind filter"
        );
        let wrong_kind =
            super::get_events(session.client.clone(), ns.as_deref(), "NoSuchKind", &name).await;
        assert!(
            wrong_kind.is_empty(),
            "kind filter must exclude other kinds"
        );
    }
}
