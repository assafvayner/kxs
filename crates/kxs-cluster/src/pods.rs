use futures::StreamExt;
use k8s_openapi::api::core::v1::{Container, ContainerStatus, Pod};
use kube::api::Api;
use kube::runtime::watcher;
use kube::Client;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PodRow {
    pub key: String,
    pub name: String,
    pub namespace: String,
    pub ready: String,
    pub status: String,
    pub restarts: u32,
    pub ip: Option<String>,
    pub node: Option<String>,
    /// RFC3339; age is rendered client-side so it can tick.
    pub created: Option<String>,
    /// Summed container requests; `None` when no container sets one, so the UI
    /// can tell "no request" apart from "requests zero".
    pub cpu_request_millis: Option<i64>,
    pub mem_request_mib: Option<i64>,
}

/// Sums a resource request across the pod's containers. `None` when no
/// container declares it (or none parses).
fn sum_requests(pod: &Pod, key: &str, parse: fn(&str) -> Option<i64>) -> Option<i64> {
    let containers = pod
        .spec
        .as_ref()
        .map(|s| s.containers.as_slice())
        .unwrap_or_default();
    let mut total: Option<i64> = None;
    for c in containers {
        let q = c
            .resources
            .as_ref()
            .and_then(|r| r.requests.as_ref())
            .and_then(|m| m.get(key));
        if let Some(v) = q.and_then(|q| parse(&q.0)) {
            total = Some(total.unwrap_or(0) + v);
        }
    }
    total
}

pub fn pod_key(pod: &Pod) -> String {
    format!(
        "{}/{}",
        pod.metadata.namespace.as_deref().unwrap_or_default(),
        pod.metadata.name.as_deref().unwrap_or_default()
    )
}

/// kubectl-printer-style status summary (simplified but covering the common
/// states: waiting reasons, init progress, Terminating, Completed, Evicted).
pub fn pod_row(pod: &Pod) -> PodRow {
    let meta = &pod.metadata;
    let name = meta.name.clone().unwrap_or_default();
    let namespace = meta.namespace.clone().unwrap_or_default();
    let status = pod.status.as_ref();
    let container_statuses = status
        .and_then(|s| s.container_statuses.as_deref())
        .unwrap_or(&[]);
    let total = pod.spec.as_ref().map(|s| s.containers.len()).unwrap_or(0);
    let ready_count = container_statuses.iter().filter(|c| c.ready).count();
    let restarts: u32 = container_statuses
        .iter()
        .map(|c| c.restart_count as u32)
        .sum();

    let phase = status
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "Unknown".into());
    let mut display = status
        .and_then(|s| s.reason.clone())
        .unwrap_or(phase.clone());
    if phase == "Succeeded" && display == "Succeeded" {
        display = "Completed".into();
    }

    if let Some(ics) = status.and_then(|s| s.init_container_statuses.as_deref()) {
        let total_init = pod
            .spec
            .as_ref()
            .and_then(|s| s.init_containers.as_ref())
            .map(|v| v.len())
            .unwrap_or(ics.len());
        let mut done = 0usize;
        for ic in ics {
            let state = ic.state.as_ref();
            if let Some(t) = state.and_then(|s| s.terminated.as_ref()) {
                if t.exit_code == 0 {
                    done += 1;
                    continue;
                }
                display = format!(
                    "Init:{}",
                    t.reason
                        .clone()
                        .unwrap_or_else(|| format!("ExitCode:{}", t.exit_code))
                );
                done = usize::MAX;
                break;
            }
            if let Some(w) = state.and_then(|s| s.waiting.as_ref()) {
                if let Some(r) = w.reason.as_deref() {
                    if r != "PodInitializing" {
                        display = format!("Init:{r}");
                        done = usize::MAX;
                        break;
                    }
                }
            }
            display = format!("Init:{done}/{total_init}");
            done = usize::MAX;
            break;
        }
        if done != usize::MAX && done < total_init {
            display = format!("Init:{done}/{total_init}");
        }
    }

    if !display.starts_with("Init:") {
        for c in container_statuses {
            if let Some(w) = c.state.as_ref().and_then(|s| s.waiting.as_ref()) {
                if let Some(r) = w.reason.clone() {
                    display = r;
                    break;
                }
            }
        }
    }

    if meta.deletion_timestamp.is_some() {
        display = "Terminating".into();
    }

    PodRow {
        key: format!("{namespace}/{name}"),
        name,
        namespace,
        ready: format!("{ready_count}/{total}"),
        status: display,
        restarts,
        ip: status.and_then(|s| s.pod_ip.clone()),
        node: pod.spec.as_ref().and_then(|s| s.node_name.clone()),
        created: meta.creation_timestamp.as_ref().map(|t| t.0.to_rfc3339()),
        cpu_request_millis: sum_requests(pod, "cpu", crate::quantity::cpu_millis),
        mem_request_mib: sum_requests(pod, "memory", crate::quantity::mem_mib),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContainerPortInfo {
    pub name: Option<String>,
    pub container_port: u16,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContainerInfo {
    pub name: String,
    pub image: String,
    pub ready: bool,
    /// Container state: "running", "waiting", "terminated", or "" when unknown.
    pub state: String,
    pub restarts: i32,
    pub ports: Vec<ContainerPortInfo>,
    pub init_container: bool,
}

/// Per-container spec+status view, init containers first (same order as
/// `logs::list_containers`).
pub fn container_infos(pod: &Pod) -> Vec<ContainerInfo> {
    let Some(spec) = pod.spec.as_ref() else {
        return Vec::new();
    };
    let status = pod.status.as_ref();
    let one = |c: &Container, init: bool, statuses: Option<&[ContainerStatus]>| {
        let st = statuses.and_then(|v| v.iter().find(|s| s.name == c.name));
        let state = st
            .and_then(|s| {
                s.state.as_ref().map(|s| {
                    if s.running.is_some() {
                        "running"
                    } else if s.terminated.is_some() {
                        "terminated"
                    } else {
                        "waiting"
                    }
                })
            })
            .unwrap_or("");
        ContainerInfo {
            name: c.name.clone(),
            image: c.image.clone().unwrap_or_default(),
            ready: st.map(|s| s.ready).unwrap_or(false),
            state: state.into(),
            restarts: st.map(|s| s.restart_count).unwrap_or(0),
            ports: c
                .ports
                .as_deref()
                .unwrap_or(&[])
                .iter()
                // containerPort is i32 in the API but always a valid TCP/UDP port
                .filter(|p| (1..=u16::MAX as i32).contains(&p.container_port))
                .map(|p| ContainerPortInfo {
                    name: p.name.clone(),
                    container_port: p.container_port as u16,
                })
                .collect(),
            init_container: init,
        }
    };
    let init_statuses = status.and_then(|s| s.init_container_statuses.as_deref());
    let statuses = status.and_then(|s| s.container_statuses.as_deref());
    spec.init_containers
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|c| one(c, true, init_statuses))
        .chain(spec.containers.iter().map(|c| one(c, false, statuses)))
        .collect()
}

pub async fn list_container_info(
    client: Client,
    namespace: &str,
    pod: &str,
) -> Result<Vec<ContainerInfo>, String> {
    let api: Api<Pod> = Api::namespaced(client, namespace);
    let p = api.get(pod).await.map_err(|e| e.to_string())?;
    Ok(container_infos(&p))
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PodEvent {
    Snapshot {
        rows: Vec<PodRow>,
    },
    Upsert {
        rows: Vec<PodRow>,
    },
    Delete {
        keys: Vec<String>,
    },
    Status {
        state: String,
        message: Option<String>,
    },
}

/// Coalesces watcher churn between flush ticks so the frontend gets bounded,
/// deduplicated batches instead of per-event traffic.
#[derive(Default)]
pub struct Batcher {
    upserts: BTreeMap<String, PodRow>,
    deletes: BTreeSet<String>,
}

impl Batcher {
    pub fn upsert(&mut self, row: PodRow) {
        self.deletes.remove(&row.key);
        self.upserts.insert(row.key.clone(), row);
    }

    pub fn delete(&mut self, key: String) {
        self.upserts.remove(&key);
        self.deletes.insert(key);
    }

    pub fn flush(&mut self) -> Vec<PodEvent> {
        let mut out = Vec::new();
        if !self.upserts.is_empty() {
            out.push(PodEvent::Upsert {
                rows: std::mem::take(&mut self.upserts).into_values().collect(),
            });
        }
        if !self.deletes.is_empty() {
            out.push(PodEvent::Delete {
                keys: std::mem::take(&mut self.deletes).into_iter().collect(),
            });
        }
        out
    }
}

/// Watches pods and pushes batched events through `send` until `send` returns
/// false (receiver gone) or `stop` fires. watcher() relists internally on
/// errors; error events surface as Status("reconnecting").
pub async fn run_pod_watch(
    client: Client,
    namespace: Option<String>,
    label_selector: Option<String>,
    send: impl Fn(PodEvent) -> bool + Send + 'static,
    stop: tokio::sync::oneshot::Receiver<()>,
) {
    let api: Api<Pod> = match namespace.as_deref() {
        Some(ns) if !ns.is_empty() => Api::namespaced(client, ns),
        _ => Api::all(client),
    };
    let mut config = watcher::Config::default();
    if let Some(sel) = label_selector.as_deref().filter(|s| !s.is_empty()) {
        config = config.labels(sel);
    }
    let stream = watcher(api, config).boxed();
    drive_pod_events(stream, send, stop).await;
}

/// Consumes a stream of watcher events into batched PodEvents. Separated from
/// the kube wiring so the reconnect logic is unit-testable with a synthetic stream.
pub async fn drive_pod_events<S>(
    mut stream: S,
    send: impl Fn(PodEvent) -> bool + Send + 'static,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) where
    S: futures::Stream<Item = Result<watcher::Event<Pod>, watcher::Error>> + Unpin,
{
    let mut batcher = Batcher::default();
    let mut init_buffer: Option<Vec<PodRow>> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = &mut stop => return,
            _ = tick.tick() => {
                for ev in batcher.flush() {
                    if !send(ev) { return; }
                }
            }
            item = stream.next() => match item {
                Some(Ok(watcher::Event::Init)) => {
                    init_buffer = Some(Vec::new());
                    batcher = Batcher::default(); // drop stale pre-reconnect deltas; the snapshot is authoritative
                }
                Some(Ok(watcher::Event::InitApply(p))) => {
                    if let Some(buf) = &mut init_buffer { buf.push(pod_row(&p)); }
                }
                Some(Ok(watcher::Event::InitDone)) => {
                    let rows = init_buffer.take().unwrap_or_default();
                    if !send(PodEvent::Snapshot { rows }) { return; }
                    if !send(PodEvent::Status { state: "live".into(), message: None }) { return; }
                }
                Some(Ok(watcher::Event::Apply(p))) => batcher.upsert(pod_row(&p)),
                Some(Ok(watcher::Event::Delete(p))) => batcher.delete(pod_key(&p)),
                Some(Err(e)) => {
                    if !send(PodEvent::Status { state: "reconnecting".into(), message: Some(e.to_string()) }) { return; }
                }
                None => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::Pod;

    fn pod(v: serde_json::Value) -> Pod {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn running_pod() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "web-1", "namespace": "app", "creationTimestamp": "2026-07-01T00:00:00Z"},
            "spec": {"containers": [{"name": "a"}, {"name": "b"}], "nodeName": "node-1"},
            "status": {"phase": "Running", "podIP": "10.0.0.5", "containerStatuses": [
                {"name": "a", "ready": true, "restartCount": 2, "state": {"running": {}}},
                {"name": "b", "ready": false, "restartCount": 0, "state": {"running": {}}}
            ]}
        }));
        let r = pod_row(&p);
        assert_eq!(r.key, "app/web-1");
        assert_eq!(r.ready, "1/2");
        assert_eq!(r.status, "Running");
        assert_eq!(r.restarts, 2);
        assert_eq!(r.ip.as_deref(), Some("10.0.0.5"));
        assert_eq!(r.node.as_deref(), Some("node-1"));
        assert_eq!(r.created.as_deref(), Some("2026-07-01T00:00:00+00:00"));
        assert_eq!(r.cpu_request_millis, None);
        assert_eq!(r.mem_request_mib, None);
    }

    #[test]
    fn sums_container_requests() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [
                {"name": "a", "resources": {"requests": {"cpu": "250m", "memory": "128Mi"}}},
                {"name": "b", "resources": {"requests": {"cpu": "1", "memory": "1Gi"}}}
            ]}
        }));
        let r = pod_row(&p);
        assert_eq!(r.cpu_request_millis, Some(1250));
        assert_eq!(r.mem_request_mib, Some(1152));
    }

    #[test]
    fn partial_and_absent_requests() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [
                {"name": "a", "resources": {"requests": {"memory": "512M"}}},
                {"name": "b", "resources": {"limits": {"cpu": "2"}}},
                {"name": "c"}
            ]}
        }));
        let r = pod_row(&p);
        assert_eq!(r.cpu_request_millis, None, "limits are not requests");
        assert_eq!(r.mem_request_mib, Some(488));
    }

    #[test]
    fn zero_request_is_some_zero() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [
                {"name": "a", "resources": {"requests": {"cpu": "0", "memory": "0"}}}
            ]}
        }));
        let r = pod_row(&p);
        assert_eq!(r.cpu_request_millis, Some(0));
        assert_eq!(r.mem_request_mib, Some(0));
    }

    #[test]
    fn waiting_reason_wins() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [{"name": "a"}]},
            "status": {"phase": "Running", "containerStatuses": [
                {"name": "a", "ready": false, "restartCount": 14,
                 "state": {"waiting": {"reason": "CrashLoopBackOff"}}}
            ]}
        }));
        assert_eq!(pod_row(&p).status, "CrashLoopBackOff");
    }

    #[test]
    fn init_containers_take_precedence() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [{"name": "a"}], "initContainers": [{"name": "i0"}, {"name": "i1"}]},
            "status": {"phase": "Pending", "initContainerStatuses": [
                {"name": "i0", "ready": false, "restartCount": 0, "state": {"running": {}}},
                {"name": "i1", "ready": false, "restartCount": 0, "state": {"waiting": {"reason": "PodInitializing"}}}
            ]}
        }));
        assert_eq!(pod_row(&p).status, "Init:0/2");
    }

    #[test]
    fn init_crash_reason_shown() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [{"name": "a"}], "initContainers": [{"name": "i0"}]},
            "status": {"phase": "Pending", "initContainerStatuses": [
                {"name": "i0", "ready": false, "restartCount": 3,
                 "state": {"waiting": {"reason": "CrashLoopBackOff"}}}
            ]}
        }));
        assert_eq!(pod_row(&p).status, "Init:CrashLoopBackOff");
    }

    #[test]
    fn terminating_overrides() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d", "deletionTimestamp": "2026-07-03T00:00:00Z"},
            "spec": {"containers": [{"name": "a"}]},
            "status": {"phase": "Running", "containerStatuses": [
                {"name": "a", "ready": true, "restartCount": 0, "state": {"running": {}}}
            ]}
        }));
        assert_eq!(pod_row(&p).status, "Terminating");
    }

    #[test]
    fn succeeded_shows_completed() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "job-1", "namespace": "d"},
            "spec": {"containers": [{"name": "a"}]},
            "status": {"phase": "Succeeded"}
        }));
        assert_eq!(pod_row(&p).status, "Completed");
    }

    #[test]
    fn evicted_reason_shown() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [{"name": "a"}]},
            "status": {"phase": "Failed", "reason": "Evicted"}
        }));
        assert_eq!(pod_row(&p).status, "Evicted");
    }

    #[test]
    fn container_creating_shown() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [{"name": "a"}]},
            "status": {"phase": "Pending", "containerStatuses": [
                {"name": "a", "ready": false, "restartCount": 0,
                 "state": {"waiting": {"reason": "ContainerCreating"}}}
            ]}
        }));
        assert_eq!(pod_row(&p).status, "ContainerCreating");
    }

    #[test]
    fn bare_pod_defaults() {
        let p = pod(serde_json::json!({"metadata": {"name": "x", "namespace": "d"}}));
        let r = pod_row(&p);
        assert_eq!(r.ready, "0/0");
        assert_eq!(r.status, "Unknown");
        assert_eq!(r.restarts, 0);
    }

    #[test]
    fn container_infos_maps_spec_and_status() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "web-1", "namespace": "app"},
            "spec": {
                "initContainers": [{"name": "migrate", "image": "migrate:1"}],
                "containers": [
                    {"name": "web", "image": "nginx:1.27", "ports": [
                        {"name": "http", "containerPort": 8080},
                        {"containerPort": 9090}
                    ]},
                    {"name": "sidecar", "image": "envoy:1.30"}
                ]
            },
            "status": {"phase": "Running",
                "initContainerStatuses": [
                    {"name": "migrate", "ready": false, "restartCount": 1, "state": {"terminated": {"exitCode": 0}}}
                ],
                "containerStatuses": [
                    {"name": "web", "ready": true, "restartCount": 3, "state": {"running": {}}},
                    {"name": "sidecar", "ready": false, "restartCount": 0, "state": {"running": {}}}
                ]}
        }));
        let infos = container_infos(&p);
        assert_eq!(
            infos.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["migrate", "web", "sidecar"],
            "init containers come first"
        );
        assert!(infos[0].init_container);
        assert_eq!(infos[0].restarts, 1);
        assert!(!infos[0].ready);
        assert!(infos[0].ports.is_empty());
        assert!(!infos[1].init_container);
        assert_eq!(infos[1].image, "nginx:1.27");
        assert!(infos[1].ready);
        assert_eq!(infos[1].restarts, 3);
        assert_eq!(
            infos[1].ports,
            vec![
                ContainerPortInfo {
                    name: Some("http".into()),
                    container_port: 8080
                },
                ContainerPortInfo {
                    name: None,
                    container_port: 9090
                }
            ]
        );
        assert_eq!(infos[2].name, "sidecar");
    }

    #[test]
    fn container_infos_without_status_defaults() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [{"name": "a", "ports": [{"containerPort": 0}, {"containerPort": 70000}]}]}
        }));
        let infos = container_infos(&p);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].image, "");
        assert!(!infos[0].ready);
        assert_eq!(infos[0].restarts, 0);
        assert!(infos[0].ports.is_empty(), "out-of-range ports dropped");
    }

    #[test]
    fn container_infos_no_spec_is_empty() {
        let p = pod(serde_json::json!({"metadata": {"name": "x", "namespace": "d"}}));
        assert!(container_infos(&p).is_empty());
    }

    fn row(key: &str) -> PodRow {
        PodRow {
            key: key.into(),
            name: key.rsplit('/').next().unwrap().into(),
            namespace: key.split('/').next().unwrap().into(),
            ready: "1/1".into(),
            status: "Running".into(),
            restarts: 0,
            ip: None,
            node: None,
            created: None,
            cpu_request_millis: None,
            mem_request_mib: None,
        }
    }

    #[test]
    fn batcher_coalesces() {
        let mut b = Batcher::default();
        assert!(b.flush().is_empty());
        b.upsert(row("d/a"));
        b.upsert(row("d/a"));
        b.upsert(row("d/b"));
        b.delete("d/c".into());
        let events = b.flush();
        assert_eq!(events.len(), 2);
        match &events[0] {
            PodEvent::Upsert { rows } => assert_eq!(rows.len(), 2),
            other => panic!("expected upsert, got {other:?}"),
        }
        match &events[1] {
            PodEvent::Delete { keys } => assert_eq!(keys, &vec!["d/c".to_string()]),
            other => panic!("expected delete, got {other:?}"),
        }
        assert!(b.flush().is_empty(), "flush must clear");
    }

    #[test]
    fn upsert_then_delete_is_delete_only() {
        let mut b = Batcher::default();
        b.upsert(row("d/a"));
        b.delete("d/a".into());
        let events = b.flush();
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], PodEvent::Delete { keys } if keys == &vec!["d/a".to_string()])
        );
    }

    #[test]
    fn delete_then_upsert_is_upsert_only() {
        let mut b = Batcher::default();
        b.delete("d/a".into());
        b.upsert(row("d/a"));
        let events = b.flush();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], PodEvent::Upsert { .. }));
    }

    #[tokio::test]
    async fn reconnect_snapshot_discards_stale_batched_events() {
        use futures::stream;
        fn p(ns: &str, name: &str) -> Pod {
            serde_json::from_value(serde_json::json!({
                "metadata": {"name": name, "namespace": ns},
                "spec": {"containers": [{"name": "c"}]},
                "status": {"phase": "Running", "containerStatuses": [
                    {"name": "c", "ready": true, "restartCount": 0, "state": {"running": {}}}
                ]}
            }))
            .unwrap()
        }
        // gen 1: foo live. then an Apply for stale/foo arrives (queued in batcher),
        // then reconnect: Init/InitApply(bar)/InitDone — snapshot has only bar, not foo.
        let events: Vec<Result<watcher::Event<Pod>, watcher::Error>> = vec![
            Ok(watcher::Event::Init),
            Ok(watcher::Event::InitApply(p("d", "foo"))),
            Ok(watcher::Event::InitDone),
            Ok(watcher::Event::Apply(p("d", "stale"))), // queued in batcher, not yet flushed
            Ok(watcher::Event::Init),                   // reconnect starts
            Ok(watcher::Event::InitApply(p("d", "bar"))),
            Ok(watcher::Event::InitDone),
        ];
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        // Chain a pending tail so the stream never ends: the loop stays alive long
        // enough for a flush tick to fire, which is exactly when a stale batched
        // upsert would leak past the reconnect snapshot if the batcher wasn't cleared.
        let src = stream::iter(events).chain(stream::pending());
        let handle = tokio::spawn(drive_pod_events(
            src,
            move |ev| tx.send(ev).is_ok(),
            stop_rx,
        ));
        // collect events for a short window (longer than one 250ms tick), then stop
        let mut got = Vec::new();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(600);
        while let Ok(Some(ev)) = tokio::time::timeout_at(deadline, rx.recv()).await {
            got.push(ev);
        }
        let _ = stop_tx.send(());
        let _ = handle.await;
        // the "stale" pod (queued before the reconnect Init) must NOT appear in any Upsert
        let leaked = got.iter().any(
            |e| matches!(e, PodEvent::Upsert { rows } if rows.iter().any(|r| r.name == "stale")),
        );
        assert!(
            !leaked,
            "stale pre-reconnect row leaked past the snapshot: {got:?}"
        );
        // sanity: last snapshot contains bar
        let last_snap = got
            .iter()
            .rev()
            .find_map(|e| match e {
                PodEvent::Snapshot { rows } => Some(rows.clone()),
                _ => None,
            })
            .expect("a snapshot");
        assert!(last_snap.iter().any(|r| r.name == "bar"));
    }

    async fn kind_session() -> crate::session::ClusterSession {
        let paths = kxs_core::kubeconfig::paths::kubeconfig_paths();
        let store = kxs_core::kubeconfig::store::KubeconfigStore::load(paths).unwrap();
        let yaml = crate::bridge::kubeconfig_yaml_for_context(&store, "kind-local").unwrap();
        crate::session::connect(&yaml, "kind-local").await.unwrap()
    }

    /// Run manually: cargo test -p kxs-cluster -- --ignored (needs kind-local).
    #[tokio::test]
    #[ignore]
    async fn list_container_info_reads_demo_pod_on_kind_local() {
        let s = kind_session().await;
        let infos = list_container_info(s.client.clone(), "default", "demo-standalone")
            .await
            .unwrap();
        assert!(!infos.is_empty(), "demo-standalone should have containers");
        assert!(infos
            .iter()
            .all(|c| !c.name.is_empty() && !c.image.is_empty()));

        // demo-web's pods declare port 80
        let pods = crate::workloads::workload_pods(
            s.client.clone(),
            "apps",
            "v1",
            "Deployment",
            "deployments",
            "default",
            "demo-web",
        )
        .await
        .unwrap();
        let pod = pods.first().expect("demo-web should have pods");
        let infos = list_container_info(s.client.clone(), "default", pod)
            .await
            .unwrap();
        assert!(
            infos
                .iter()
                .any(|c| c.ports.iter().any(|p| p.container_port == 80)),
            "{infos:?}"
        );
    }

    /// Run manually: cargo test -p kxs-cluster -- --ignored (needs kind-local in ~/.kube/config)
    #[tokio::test]
    #[ignore]
    async fn watches_kind_local_pods() {
        let session = kind_session().await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (_stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(run_pod_watch(
            session.client.clone(),
            None,
            None,
            move |ev| tx.send(ev).is_ok(),
            stop_rx,
        ));
        let first = tokio::time::timeout(Duration::from_secs(15), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(first, PodEvent::Snapshot { .. }));
    }
}
