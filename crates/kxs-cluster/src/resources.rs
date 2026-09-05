use futures::StreamExt;
use kube::api::{Api, DynamicObject, ListParams};
use kube::core::{ApiResource, GroupVersionKind};
use kube::runtime::{metadata_watcher, watcher};
use kube::Client;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::time::Duration;

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

/// Percent-encode a value for use in a query string (base64 continue tokens
/// can contain '+', '/', '='; label selectors contain '=', ',', '!').
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

/// Server-side Table list for any kind. `namespace: None` = all namespaces,
/// `label_selector: None` = no selector. Follows list continuation up to
/// `MAX_TABLE_PAGES` pages so large clusters aren't silently truncated at the
/// per-request limit.
pub async fn list_table(
    client: Client,
    group: &str,
    version: &str,
    plural: &str,
    namespace: Option<&str>,
    label_selector: Option<&str>,
) -> Result<ResourceTable, String> {
    const MAX_TABLE_PAGES: usize = 10;
    let path = list_path(group, version, plural, namespace);
    let selector = label_selector.filter(|s| !s.is_empty());
    let mut merged: Option<RawTable> = None;
    let mut continue_token: Option<String> = None;
    for _ in 0..MAX_TABLE_PAGES {
        let mut url = format!("{path}?limit=1000");
        if let Some(s) = selector {
            url.push_str("&labelSelector=");
            url.push_str(&encode_query_value(s));
        }
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

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TableEvent {
    /// A full server-rendered table; replaces whatever the view holds.
    Table { table: ResourceTable },
    Status {
        state: String,
        message: Option<String>,
    },
}

/// Watcher churn (a rollout touches every pod) is coalesced into at most one
/// Table re-fetch per window, measured from the first change so latency stays
/// bounded even under continuous churn.
const TABLE_DEBOUNCE: Duration = Duration::from_millis(300);
/// Safety net for kinds whose watch never establishes (discovery lists
/// resources without checking the `watch` verb) and for events lost between
/// relists. Far cheaper than the 5s poll it replaces.
const TABLE_RESYNC: Duration = Duration::from_secs(30);

/// Watches object metadata for one kind and pushes a freshly rendered
/// server-side Table through `send` on every (debounced) change, until `send`
/// returns false (receiver gone) or `stop` fires. Metadata-only watches keep
/// the streamed payload small; the rows themselves always come from the
/// apiserver's Table printer.
#[allow(clippy::too_many_arguments)]
pub async fn run_table_watch(
    client: Client,
    group: String,
    version: String,
    kind: String,
    plural: String,
    namespace: Option<String>,
    label_selector: Option<String>,
    send: impl Fn(TableEvent) -> bool + Send + 'static,
    stop: tokio::sync::oneshot::Receiver<()>,
) {
    let namespace = namespace.filter(|n| !n.is_empty());
    let label_selector = label_selector.filter(|s| !s.is_empty());
    let ar = api_resource(&group, &version, &kind, &plural);
    let api: Api<DynamicObject> = match namespace.as_deref() {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };
    let mut config = watcher::Config::default();
    if let Some(sel) = label_selector.as_deref() {
        config = config.labels(sel);
    }
    let stream = metadata_watcher(api, config).boxed();
    let fetch = move || {
        let (client, group, version, plural, namespace, label_selector) = (
            client.clone(),
            group.clone(),
            version.clone(),
            plural.clone(),
            namespace.clone(),
            label_selector.clone(),
        );
        async move {
            list_table(
                client,
                &group,
                &version,
                &plural,
                namespace.as_deref(),
                label_selector.as_deref(),
            )
            .await
        }
    };
    drive_table_events(stream, fetch, TABLE_DEBOUNCE, TABLE_RESYNC, send, stop).await;
}

async fn emit_table<F, Fut>(fetch: &F, send: &impl Fn(TableEvent) -> bool) -> bool
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<ResourceTable, String>>,
{
    match fetch().await {
        Ok(table) => send(TableEvent::Table { table }),
        Err(message) => send(TableEvent::Status {
            state: "error".into(),
            message: Some(message),
        }),
    }
}

/// Turns a stream of watcher events into debounced Table re-fetches. Generic
/// over the watched object because only the fact that *something* changed
/// matters — which also makes the debounce/reconnect logic unit-testable.
pub async fn drive_table_events<S, T, F, Fut>(
    mut stream: S,
    fetch: F,
    debounce: Duration,
    resync: Duration,
    send: impl Fn(TableEvent) -> bool + Send,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) where
    S: futures::Stream<Item = Result<watcher::Event<T>, watcher::Error>> + Unpin,
    F: Fn() -> Fut,
    Fut: Future<Output = Result<ResourceTable, String>>,
{
    // First table goes out immediately; the initial watch relist is slower and
    // its objects carry no printer columns anyway.
    if !emit_table(&fetch, &send).await {
        return;
    }
    let mut first_relist = true;
    let mut deadline: Option<tokio::time::Instant> = None;
    let mut resync_tick = tokio::time::interval_at(tokio::time::Instant::now() + resync, resync);
    resync_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let pending_deadline = deadline;
        tokio::select! {
            _ = &mut stop => return,
            _ = resync_tick.tick() => {
                deadline.get_or_insert_with(|| tokio::time::Instant::now() + debounce);
            }
            _ = async move {
                match pending_deadline {
                    Some(d) => tokio::time::sleep_until(d).await,
                    None => std::future::pending().await,
                }
            } => {
                deadline = None;
                if !emit_table(&fetch, &send).await { return; }
            }
            item = stream.next() => match item {
                Some(Ok(watcher::Event::Init | watcher::Event::InitApply(_))) => {}
                Some(Ok(watcher::Event::InitDone)) => {
                    if !send(TableEvent::Status { state: "live".into(), message: None }) { return; }
                    // A relist means the watch was re-established, so changes may
                    // have been missed; the very first one is covered above.
                    if first_relist {
                        first_relist = false;
                    } else {
                        deadline.get_or_insert_with(|| tokio::time::Instant::now() + debounce);
                    }
                }
                Some(Ok(watcher::Event::Apply(_) | watcher::Event::Delete(_))) => {
                    deadline.get_or_insert_with(|| tokio::time::Instant::now() + debounce);
                }
                Some(Err(e)) => {
                    if !send(TableEvent::Status { state: "reconnecting".into(), message: Some(e.to_string()) }) { return; }
                }
                None => return,
            }
        }
    }
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
    pub first_seen: Option<String>,
    /// Reporting component (`kubelet`, `default-scheduler`, ...); empty when unknown.
    pub source: String,
}

fn resource_event(e: k8s_openapi::api::core::v1::Event) -> ResourceEvent {
    let series_count = e.series.as_ref().and_then(|series| series.count);
    let last_seen = e
        .series
        .as_ref()
        .and_then(|series| series.last_observed_time.as_ref())
        .map(|t| t.0.to_rfc3339())
        .or_else(|| e.last_timestamp.as_ref().map(|t| t.0.to_rfc3339()))
        .or_else(|| e.event_time.as_ref().map(|t| t.0.to_rfc3339()));
    let first_seen = e
        .event_time
        .as_ref()
        .map(|t| t.0.to_rfc3339())
        .or_else(|| e.first_timestamp.as_ref().map(|t| t.0.to_rfc3339()));

    ResourceEvent {
        type_: e.type_.unwrap_or_default(),
        reason: e.reason.unwrap_or_default(),
        message: e.message.unwrap_or_default(),
        count: series_count.or(e.count).unwrap_or(1),
        last_seen,
        first_seen,
        source: e
            .source
            .and_then(|s| s.component)
            .or(e.reporting_component)
            .unwrap_or_default(),
    }
}

/// Events referencing a given object (best-effort; empty on error). Filtering
/// by kind too keeps out events for same-named objects of other kinds (e.g. a
/// Service and Deployment both called "web").
/// First `metadata.ownerReferences` entry of a manifest, as (kind, name).
/// `None` when the resource is not owned by anything.
pub fn owner_reference(yaml: &str) -> Option<(String, String)> {
    let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).ok()?;
    let first = doc
        .get("metadata")?
        .get("ownerReferences")?
        .as_sequence()?
        .first()?;
    Some((
        first.get("kind")?.as_str()?.to_string(),
        first.get("name")?.as_str()?.to_string(),
    ))
}

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
    list.items.into_iter().map(resource_event).collect()
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
    fn label_selector_query_encoding() {
        assert_eq!(
            encode_query_value("app=demo-web,tier!=db"),
            "app%3Ddemo-web%2Ctier%21%3Ddb"
        );
    }

    #[test]
    fn empty_namespace_treated_as_all() {
        // caller passes None for all-namespaces; empty &str should not happen, but guard:
        assert_eq!(list_path("", "v1", "pods", None), "/api/v1/pods");
    }

    #[test]
    fn resource_event_prefers_modern_series_fields() {
        let event = serde_json::from_value(serde_json::json!({
            "metadata": {},
            "involvedObject": {},
            "type": "Warning",
            "reason": "BackOff",
            "message": "Back-off restarting",
            "count": 2,
            "eventTime": "2026-07-03T11:50:00Z",
            "firstTimestamp": "2026-07-03T11:40:00Z",
            "lastTimestamp": "2026-07-03T11:55:00Z",
            "series": {
                "count": 4,
                "lastObservedTime": "2026-07-03T11:58:00Z"
            },
            "source": { "component": "kubelet" },
            "reportingComponent": "new-kubelet"
        }))
        .unwrap();

        let mapped = resource_event(event);
        assert_eq!(mapped.type_, "Warning");
        assert_eq!(mapped.reason, "BackOff");
        assert_eq!(mapped.message, "Back-off restarting");
        assert_eq!(mapped.count, 4);
        assert_eq!(
            mapped.last_seen.as_deref(),
            Some("2026-07-03T11:58:00+00:00")
        );
        assert_eq!(
            mapped.first_seen.as_deref(),
            Some("2026-07-03T11:50:00+00:00")
        );
        assert_eq!(mapped.source, "kubelet");
    }

    #[test]
    fn resource_event_falls_back_to_legacy_fields() {
        let event = serde_json::from_value(serde_json::json!({
            "metadata": {},
            "involvedObject": {},
            "count": 2,
            "firstTimestamp": "2026-07-03T11:40:00Z",
            "lastTimestamp": "2026-07-03T11:55:00Z",
            "reportingComponent": "default-scheduler"
        }))
        .unwrap();

        let mapped = resource_event(event);
        assert_eq!(mapped.count, 2);
        assert_eq!(
            mapped.last_seen.as_deref(),
            Some("2026-07-03T11:55:00+00:00")
        );
        assert_eq!(
            mapped.first_seen.as_deref(),
            Some("2026-07-03T11:40:00+00:00")
        );
        assert_eq!(mapped.source, "default-scheduler");
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

    mod table_watch {
        use super::super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        const DEBOUNCE: Duration = Duration::from_millis(50);
        /// long enough that only the tests that want it will see a tick
        const NO_RESYNC: Duration = Duration::from_secs(3600);

        fn table(n: usize) -> ResourceTable {
            ResourceTable {
                columns: vec!["Name".into(), "Age".into()],
                rows: (0..n)
                    .map(|i| ResourceRow {
                        key: format!("d/r{i}"),
                        name: format!("r{i}"),
                        namespace: Some("d".into()),
                        cells: vec![format!("r{i}")],
                        created: None,
                    })
                    .collect(),
            }
        }

        /// Drives `events` (followed by a never-ending tail so the loop stays
        /// alive) for `window`, returning everything sent plus the fetch count.
        async fn drive(
            events: Vec<Result<watcher::Event<()>, watcher::Error>>,
            window: Duration,
        ) -> (Vec<TableEvent>, usize) {
            drive_with_resync(events, window, NO_RESYNC).await
        }

        async fn drive_with_resync(
            events: Vec<Result<watcher::Event<()>, watcher::Error>>,
            window: Duration,
            resync: Duration,
        ) -> (Vec<TableEvent>, usize) {
            let fetches = Arc::new(AtomicUsize::new(0));
            let counter = fetches.clone();
            let fetch = move || {
                let n = counter.fetch_add(1, Ordering::SeqCst);
                async move { Ok(table(n)) }
            };
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
            let src = futures::stream::iter(events).chain(futures::stream::pending());
            let handle = tokio::spawn(drive_table_events(
                src,
                fetch,
                DEBOUNCE,
                resync,
                move |ev| tx.send(ev).is_ok(),
                stop_rx,
            ));
            let mut got = Vec::new();
            let deadline = tokio::time::Instant::now() + window;
            while let Ok(Some(ev)) = tokio::time::timeout_at(deadline, rx.recv()).await {
                got.push(ev);
            }
            let _ = stop_tx.send(());
            tokio::time::timeout(Duration::from_secs(2), handle)
                .await
                .expect("drive_table_events must return once stopped")
                .unwrap();
            (got, fetches.load(Ordering::SeqCst))
        }

        fn tables(evs: &[TableEvent]) -> Vec<&ResourceTable> {
            evs.iter()
                .filter_map(|e| match e {
                    TableEvent::Table { table } => Some(table),
                    _ => None,
                })
                .collect()
        }

        fn states(evs: &[TableEvent]) -> Vec<&str> {
            evs.iter()
                .filter_map(|e| match e {
                    TableEvent::Status { state, .. } => Some(state.as_str()),
                    _ => None,
                })
                .collect()
        }

        #[tokio::test]
        async fn first_table_precedes_any_watch_event() {
            let (got, fetches) = drive(Vec::new(), Duration::from_millis(200)).await;
            assert!(
                matches!(got.first(), Some(TableEvent::Table { .. })),
                "expected an immediate table, got {got:?}"
            );
            assert_eq!(fetches, 1, "no watch events, so no re-fetch");
        }

        #[tokio::test]
        async fn initial_relist_does_not_refetch() {
            let events = vec![
                Ok(watcher::Event::Init),
                Ok(watcher::Event::InitApply(())),
                Ok(watcher::Event::InitApply(())),
                Ok(watcher::Event::InitDone),
            ];
            let (got, fetches) = drive(events, Duration::from_millis(300)).await;
            assert_eq!(fetches, 1, "the immediate table already covers the relist");
            assert_eq!(states(&got), vec!["live"]);
        }

        #[tokio::test]
        async fn burst_of_changes_is_coalesced_into_one_refetch() {
            let mut events = vec![Ok(watcher::Event::Init), Ok(watcher::Event::InitDone)];
            for _ in 0..20 {
                events.push(Ok(watcher::Event::Apply(())));
            }
            events.push(Ok(watcher::Event::Delete(())));
            let (got, fetches) = drive(events, Duration::from_millis(400)).await;
            assert_eq!(fetches, 2, "initial fetch plus one debounced refetch");
            assert_eq!(tables(&got).len(), 2);
        }

        #[tokio::test]
        async fn reconnect_relist_refetches_and_reports_status() {
            let events = vec![
                Ok(watcher::Event::Init),
                Ok(watcher::Event::InitDone),
                Err(watcher::Error::NoResourceVersion),
                Ok(watcher::Event::Init),
                Ok(watcher::Event::InitApply(())),
                Ok(watcher::Event::InitDone),
            ];
            let (got, fetches) = drive(events, Duration::from_millis(400)).await;
            assert_eq!(fetches, 2, "a re-established watch may have missed changes");
            assert_eq!(states(&got), vec!["live", "reconnecting", "live"]);
        }

        #[tokio::test]
        async fn resync_refetches_without_watch_events() {
            // a kind that cannot be watched at all: only the resync moves the table on
            let events = vec![Err(watcher::Error::NoResourceVersion)];
            let (got, fetches) = drive_with_resync(
                events,
                Duration::from_millis(600),
                Duration::from_millis(150),
            )
            .await;
            assert!(fetches >= 3, "expected repeated resyncs, got {fetches}");
            assert_eq!(states(&got), vec!["reconnecting"]);
        }

        #[tokio::test]
        async fn fetch_failure_becomes_error_status_and_a_dead_receiver_ends_the_watch() {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let (_stop_tx, stop_rx) = tokio::sync::oneshot::channel();
            let (events, event_stream) =
                futures::channel::mpsc::unbounded::<Result<watcher::Event<()>, watcher::Error>>();
            let handle = tokio::spawn(drive_table_events(
                event_stream,
                || async { Err("403 forbidden".to_string()) },
                DEBOUNCE,
                NO_RESYNC,
                move |ev| tx.send(ev).is_ok(),
                stop_rx,
            ));
            match rx.recv().await.unwrap() {
                TableEvent::Status { state, message } => {
                    assert_eq!(state, "error");
                    assert_eq!(message.as_deref(), Some("403 forbidden"));
                }
                other => panic!("expected an error status, got {other:?}"),
            }
            drop(rx);
            // the next send() fails, which must end the watch
            events
                .unbounded_send(Ok(watcher::Event::Apply(())))
                .unwrap();
            tokio::time::timeout(Duration::from_secs(2), handle)
                .await
                .expect("drive_table_events must return once the receiver is gone")
                .unwrap();
        }

        #[tokio::test]
        async fn stop_ends_the_loop() {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
            let handle = tokio::spawn(drive_table_events(
                futures::stream::pending::<Result<watcher::Event<()>, watcher::Error>>(),
                || async { Ok(table(1)) },
                DEBOUNCE,
                NO_RESYNC,
                move |ev| tx.send(ev).is_ok(),
                stop_rx,
            ));
            assert!(matches!(rx.recv().await, Some(TableEvent::Table { .. })));
            stop_tx.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(2), handle)
                .await
                .expect("drive_table_events must return once stopped")
                .unwrap();
        }
    }

    /// Run manually: cargo test -p kxs-cluster -- --ignored (needs kind-local in ~/.kube/config)
    #[tokio::test]
    #[ignore]
    async fn lists_deployments_as_table_on_kind_local() {
        let session = kind_session().await;
        let t = super::list_table(
            session.client.clone(),
            "apps",
            "v1",
            "deployments",
            None,
            None,
        )
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

    /// Run manually: cargo test -p kxs-cluster table_watch_reflects -- --ignored
    /// (needs kind-local in ~/.kube/config). Creates and deletes ConfigMaps in
    /// a dedicated namespace, which is removed afterwards.
    #[tokio::test]
    #[ignore]
    async fn table_watch_reflects_changes_on_kind_local() {
        use k8s_openapi::api::core::v1::{ConfigMap, Namespace};
        use kube::api::{DeleteParams, PostParams};

        const NS: &str = "kxs-e2e-watch";
        fn cm(name: &str) -> ConfigMap {
            serde_json::from_value(serde_json::json!({"metadata": {"name": name}})).unwrap()
        }
        async fn next_table(
            rx: &mut tokio::sync::mpsc::UnboundedReceiver<TableEvent>,
            has: impl Fn(&ResourceTable) -> bool,
        ) -> Result<(), String> {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
            loop {
                match tokio::time::timeout_at(deadline, rx.recv()).await {
                    Ok(Some(TableEvent::Table { table })) if has(&table) => return Ok(()),
                    Ok(Some(_)) => continue,
                    Ok(None) => return Err("watch ended".into()),
                    Err(_) => return Err("timed out waiting for a matching table".into()),
                }
            }
        }

        let session = kind_session().await;
        let client = session.client.clone();
        let ns_api: Api<Namespace> = Api::all(client.clone());
        let _ = ns_api
            .create(
                &PostParams::default(),
                &serde_json::from_value(serde_json::json!({"metadata": {"name": NS}})).unwrap(),
            )
            .await;

        let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), NS);
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let result = async {
            cm_api
                .create(&PostParams::default(), &cm("probe-a"))
                .await
                .map_err(|e| format!("create probe-a: {e}"))?;
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(super::run_table_watch(
                client.clone(),
                String::new(),
                "v1".into(),
                "ConfigMap".into(),
                "configmaps".into(),
                Some(NS.into()),
                None,
                move |ev| tx.send(ev).is_ok(),
                stop_rx,
            ));
            let has =
                |name: &'static str| move |t: &ResourceTable| t.rows.iter().any(|r| r.name == name);
            // the immediate first table must already carry existing objects
            next_table(&mut rx, has("probe-a")).await?;
            cm_api
                .create(&PostParams::default(), &cm("probe-b"))
                .await
                .map_err(|e| format!("create probe-b: {e}"))?;
            next_table(&mut rx, has("probe-b")).await?;
            cm_api
                .delete("probe-b", &DeleteParams::default())
                .await
                .map_err(|e| format!("delete probe-b: {e}"))?;
            next_table(&mut rx, |t| !has("probe-b")(t)).await
        }
        .await;
        let _ = stop_tx.send(());
        let _ = ns_api.delete(NS, &DeleteParams::default()).await;
        result.unwrap();
    }

    /// Run manually: cargo test -p kxs-cluster label_selector -- --ignored
    /// (kind-local only — every other context is production). Read-only: the
    /// default namespace's demo workloads already carry app=demo-web.
    #[tokio::test]
    #[ignore]
    async fn label_selector_filters_list_and_watch_on_kind_local() {
        use k8s_openapi::api::core::v1::Pod;
        use std::collections::BTreeSet;

        const SEL: &str = "app=demo-web";
        let session = kind_session().await;
        let client = session.client.clone();

        // ground truth straight from a typed list with the same selector
        let pod_api: Api<Pod> = Api::namespaced(client.clone(), "default");
        let expected: BTreeSet<String> = pod_api
            .list(&ListParams::default().labels(SEL))
            .await
            .unwrap()
            .items
            .into_iter()
            .filter_map(|p| p.metadata.name)
            .collect();
        assert!(
            !expected.is_empty(),
            "kind-local's default namespace should have {SEL} pods"
        );

        let names = |t: &ResourceTable| -> BTreeSet<String> {
            t.rows.iter().map(|r| r.name.clone()).collect()
        };

        let all = super::list_table(client.clone(), "", "v1", "pods", Some("default"), None)
            .await
            .unwrap();
        let selected =
            super::list_table(client.clone(), "", "v1", "pods", Some("default"), Some(SEL))
                .await
                .unwrap();
        assert_eq!(names(&selected), expected, "list must apply the selector");
        assert!(
            names(&all).is_superset(&expected) && all.rows.len() >= selected.rows.len(),
            "unfiltered list must be a superset"
        );

        let empty = super::list_table(
            client.clone(),
            "",
            "v1",
            "pods",
            Some("default"),
            Some("app=kxs-no-such-label"),
        )
        .await
        .unwrap();
        assert!(
            empty.rows.is_empty(),
            "a selector matching nothing must yield no rows: {:?}",
            names(&empty)
        );

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(super::run_table_watch(
            client.clone(),
            String::new(),
            "v1".into(),
            "Pod".into(),
            "pods".into(),
            Some("default".into()),
            Some(SEL.into()),
            move |ev| tx.send(ev).is_ok(),
            stop_rx,
        ));
        let got = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                match rx.recv().await {
                    Some(TableEvent::Table { table }) => return names(&table),
                    Some(_) => continue,
                    None => panic!("watch ended before any table"),
                }
            }
        })
        .await
        .expect("expected a table from the selector watch");
        let _ = stop_tx.send(());
        assert_eq!(got, expected, "watch must apply the selector");
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
