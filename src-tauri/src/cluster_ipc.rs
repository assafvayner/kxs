use kxs_cluster::exec::{exec, ExecEvent, ExecHandle};
use kxs_cluster::pods::{run_pod_watch, PodEvent};
use kxs_cluster::session::ClusterSession;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::{oneshot, Mutex};

use crate::ipc::AppState;

#[derive(Default)]
pub struct Sessions(pub Arc<Mutex<HashMap<u32, SessionHandle>>>);

/// Log stream ids are process-global so a stream that outlives its
/// SessionHandle (replaced by a reconnect) can never prune a same-numbered
/// stop entry belonging to the replacement handle.
static NEXT_LOG_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub struct SessionHandle {
    pub session: Arc<ClusterSession>,
    pub pod_stop: Option<oneshot::Sender<()>>,
    pub log_stops: HashMap<u32, oneshot::Sender<()>>,
    pub execs: HashMap<u32, ExecHandle>,
    pub next_exec_id: u32,
    pub forwards: HashMap<u32, (ForwardTarget, oneshot::Sender<()>)>,
    pub next_forward_id: u32,
}

#[derive(Clone)]
pub struct ForwardTarget {
    pub local_port: u16,
    pub namespace: String,
    pub pod: String,
    pub pod_port: u16,
}

impl SessionHandle {
    fn stop_all(self) {
        if let Some(stop) = self.pod_stop {
            let _ = stop.send(());
        }
        for (_, stop) in self.log_stops {
            let _ = stop.send(());
        }
        for (_, handle) in self.execs {
            let _ = handle.stop.send(());
        }
        for (_, (_, stop)) in self.forwards {
            let _ = stop.send(());
        }
    }
}

fn base64_decode(data: &str) -> Result<Vec<u8>, String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    STANDARD.decode(data).map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub version: String,
    pub default_namespace: String,
}

fn yaml_for(state: &State<'_, AppState>, context: &str) -> Result<String, String> {
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    kxs_cluster::bridge::kubeconfig_yaml_for_context(&store, context)
}

async fn session_of(
    sessions: &State<'_, Sessions>,
    tab_id: u32,
) -> Result<Arc<ClusterSession>, String> {
    let map = sessions.0.lock().await;
    map.get(&tab_id)
        .map(|h| h.session.clone())
        .ok_or_else(|| "no session for tab".to_string())
}

#[tauri::command]
pub async fn open_session(
    tab_id: u32,
    context: String,
    state: State<'_, AppState>,
    sessions: State<'_, Sessions>,
) -> Result<SessionInfo, String> {
    let yaml = yaml_for(&state, &context)?;
    let session = kxs_cluster::session::connect(&yaml, &context).await?;
    let version = kxs_cluster::session::ping(&session, Duration::from_secs(10)).await?;
    let info = SessionInfo {
        version,
        default_namespace: session.default_namespace.clone(),
    };
    let mut map = sessions.0.lock().await;
    if let Some(old) = map.insert(
        tab_id,
        SessionHandle {
            session: Arc::new(session),
            pod_stop: None,
            log_stops: HashMap::new(),
            execs: HashMap::new(),
            next_exec_id: 0,
            forwards: HashMap::new(),
            next_forward_id: 0,
        },
    ) {
        old.stop_all();
    }
    Ok(info)
}

#[tauri::command]
pub async fn close_session(tab_id: u32, sessions: State<'_, Sessions>) -> Result<(), String> {
    if let Some(handle) = sessions.0.lock().await.remove(&tab_id) {
        handle.stop_all();
    }
    Ok(())
}

#[tauri::command]
pub async fn list_namespaces(
    tab_id: u32,
    sessions: State<'_, Sessions>,
) -> Result<Vec<String>, String> {
    let session = {
        let map = sessions.0.lock().await;
        map.get(&tab_id)
            .map(|h| h.session.clone())
            .ok_or("no session for tab")?
    };
    kxs_cluster::session::namespaces(&session).await
}

#[tauri::command]
pub async fn watch_pods(
    tab_id: u32,
    namespace: Option<String>,
    channel: Channel<PodEvent>,
    sessions: State<'_, Sessions>,
) -> Result<(), String> {
    let (stop_tx, stop_rx) = oneshot::channel();
    let client = {
        let mut map = sessions.0.lock().await;
        let handle = map.get_mut(&tab_id).ok_or("no session for tab")?;
        if let Some(old) = handle.pod_stop.replace(stop_tx) {
            let _ = old.send(());
        }
        handle.session.client.clone()
    };
    tauri::async_runtime::spawn(run_pod_watch(
        client,
        namespace,
        move |ev| channel.send(ev).is_ok(),
        stop_rx,
    ));
    Ok(())
}

#[tauri::command]
pub async fn list_containers(
    tab_id: u32,
    namespace: String,
    pod: String,
    sessions: State<'_, Sessions>,
) -> Result<Vec<String>, String> {
    let session = {
        let map = sessions.0.lock().await;
        map.get(&tab_id)
            .map(|h| h.session.clone())
            .ok_or("no session for tab")?
    };
    kxs_cluster::logs::list_containers(session.client.clone(), &namespace, &pod).await
}

#[tauri::command]
pub async fn stream_logs(
    tab_id: u32,
    request: kxs_cluster::logs::LogRequest,
    channel: Channel<kxs_cluster::logs::LogEvent>,
    sessions: State<'_, Sessions>,
) -> Result<u32, String> {
    let (stop_tx, stop_rx) = oneshot::channel();
    let (client, id) = {
        let mut map = sessions.0.lock().await;
        let handle = map.get_mut(&tab_id).ok_or("no session for tab")?;
        let id = NEXT_LOG_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        handle.log_stops.insert(id, stop_tx);
        (handle.session.client.clone(), id)
    };
    let map = sessions.0.clone();
    tauri::async_runtime::spawn(async move {
        kxs_cluster::logs::run_log_stream(
            client,
            request,
            move |ev| channel.send(ev).is_ok(),
            stop_rx,
        )
        .await;
        // prune our own stop entry once the stream ends (Eof/Error/stop)
        if let Some(handle) = map.lock().await.get_mut(&tab_id) {
            handle.log_stops.remove(&id);
        }
    });
    Ok(id)
}

#[tauri::command]
pub async fn stop_logs(
    tab_id: u32,
    stream_id: u32,
    sessions: State<'_, Sessions>,
) -> Result<(), String> {
    if let Some(handle) = sessions.0.lock().await.get_mut(&tab_id) {
        if let Some(stop) = handle.log_stops.remove(&stream_id) {
            let _ = stop.send(());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn list_resource_kinds(
    tab_id: u32,
    sessions: State<'_, Sessions>,
) -> Result<Vec<kxs_cluster::discovery::ResourceKind>, String> {
    let session = session_of(&sessions, tab_id).await?;
    kxs_cluster::discovery::discover(session.client.clone()).await
}

#[tauri::command]
pub async fn list_present_kinds(
    tab_id: u32,
    namespace: Option<String>,
    kinds: Vec<kxs_cluster::resources::KindProbe>,
    sessions: State<'_, Sessions>,
) -> Result<Vec<String>, String> {
    let session = session_of(&sessions, tab_id).await?;
    Ok(
        kxs_cluster::resources::present_kinds(session.client.clone(), namespace.as_deref(), kinds)
            .await,
    )
}

#[tauri::command]
pub async fn list_resource_table(
    tab_id: u32,
    group: String,
    version: String,
    plural: String,
    namespace: Option<String>,
    sessions: State<'_, Sessions>,
) -> Result<kxs_cluster::resources::ResourceTable, String> {
    let session = session_of(&sessions, tab_id).await?;
    kxs_cluster::resources::list_table(
        session.client.clone(),
        &group,
        &version,
        &plural,
        namespace.as_deref(),
    )
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn get_resource_yaml(
    tab_id: u32,
    group: String,
    version: String,
    kind: String,
    plural: String,
    namespace: Option<String>,
    name: String,
    sessions: State<'_, Sessions>,
) -> Result<String, String> {
    let session = session_of(&sessions, tab_id).await?;
    kxs_cluster::resources::get_yaml(
        session.client.clone(),
        &group,
        &version,
        &kind,
        &plural,
        namespace.as_deref(),
        &name,
    )
    .await
}

#[tauri::command]
pub async fn get_resource_events(
    tab_id: u32,
    namespace: Option<String>,
    kind: String,
    name: String,
    sessions: State<'_, Sessions>,
) -> Result<Vec<kxs_cluster::resources::ResourceEvent>, String> {
    let session = session_of(&sessions, tab_id).await?;
    Ok(kxs_cluster::resources::get_events(
        session.client.clone(),
        namespace.as_deref(),
        &kind,
        &name,
    )
    .await)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn apply_resource_yaml(
    tab_id: u32,
    group: String,
    version: String,
    kind: String,
    plural: String,
    namespace: Option<String>,
    name: String,
    yaml: String,
    dry_run: bool,
    sessions: State<'_, Sessions>,
) -> Result<(), String> {
    let session = session_of(&sessions, tab_id).await?;
    kxs_cluster::edit::apply_yaml(
        session.client.clone(),
        &group,
        &version,
        &kind,
        &plural,
        namespace.as_deref(),
        &name,
        &yaml,
        dry_run,
    )
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn delete_resource(
    tab_id: u32,
    group: String,
    version: String,
    kind: String,
    plural: String,
    namespace: Option<String>,
    name: String,
    sessions: State<'_, Sessions>,
) -> Result<(), String> {
    let session = session_of(&sessions, tab_id).await?;
    kxs_cluster::edit::delete_resource(
        session.client.clone(),
        &group,
        &version,
        &kind,
        &plural,
        namespace.as_deref(),
        &name,
    )
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn scale_resource(
    tab_id: u32,
    group: String,
    version: String,
    kind: String,
    plural: String,
    namespace: Option<String>,
    name: String,
    replicas: i32,
    sessions: State<'_, Sessions>,
) -> Result<(), String> {
    let session = session_of(&sessions, tab_id).await?;
    kxs_cluster::edit::merge_patch(
        session.client.clone(),
        &group,
        &version,
        &kind,
        &plural,
        namespace.as_deref(),
        &name,
        kxs_cluster::edit::scale_patch(replicas),
    )
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn restart_resource(
    tab_id: u32,
    group: String,
    version: String,
    kind: String,
    plural: String,
    namespace: Option<String>,
    name: String,
    restarted_at: String,
    sessions: State<'_, Sessions>,
) -> Result<(), String> {
    let session = session_of(&sessions, tab_id).await?;
    kxs_cluster::edit::merge_patch(
        session.client.clone(),
        &group,
        &version,
        &kind,
        &plural,
        namespace.as_deref(),
        &name,
        kxs_cluster::edit::restart_patch(&restarted_at),
    )
    .await
}

#[tauri::command]
pub async fn cordon_node(
    tab_id: u32,
    name: String,
    unschedulable: bool,
    sessions: State<'_, Sessions>,
) -> Result<(), String> {
    let session = session_of(&sessions, tab_id).await?;
    kxs_cluster::edit::merge_patch(
        session.client.clone(),
        "",
        "v1",
        "Node",
        "nodes",
        None,
        &name,
        kxs_cluster::edit::cordon_patch(unschedulable),
    )
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_exec(
    tab_id: u32,
    namespace: String,
    pod: String,
    container: Option<String>,
    command: Vec<String>,
    cols: u16,
    rows: u16,
    channel: Channel<ExecEvent>,
    sessions: State<'_, Sessions>,
) -> Result<u32, String> {
    let client = session_of(&sessions, tab_id).await?.client.clone();
    let handle = exec(
        client,
        &namespace,
        &pod,
        container.as_deref(),
        command,
        cols,
        rows,
        move |ev| channel.send(ev).is_ok(),
    )
    .await?;
    let mut map = sessions.0.lock().await;
    let h = map.get_mut(&tab_id).ok_or("no session for tab")?;
    let id = h.next_exec_id;
    h.next_exec_id += 1;
    h.execs.insert(id, handle);
    Ok(id)
}

#[tauri::command]
pub async fn exec_stdin(
    tab_id: u32,
    exec_id: u32,
    data: String,
    sessions: State<'_, Sessions>,
) -> Result<(), String> {
    // data is base64 (raw keystroke bytes)
    let bytes = base64_decode(&data)?;
    let map = sessions.0.lock().await;
    let h = map.get(&tab_id).ok_or("no session for tab")?;
    let ex = h.execs.get(&exec_id).ok_or("no exec")?;
    ex.stdin.send(bytes).map_err(|_| "exec closed".to_string())
}

#[tauri::command]
pub async fn exec_resize(
    tab_id: u32,
    exec_id: u32,
    cols: u16,
    rows: u16,
    sessions: State<'_, Sessions>,
) -> Result<(), String> {
    let map = sessions.0.lock().await;
    if let Some(h) = map.get(&tab_id) {
        if let Some(ex) = h.execs.get(&exec_id) {
            let _ = ex.resize.send((cols, rows));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn stop_exec(
    tab_id: u32,
    exec_id: u32,
    sessions: State<'_, Sessions>,
) -> Result<(), String> {
    let mut map = sessions.0.lock().await;
    if let Some(h) = map.get_mut(&tab_id) {
        if let Some(ex) = h.execs.remove(&exec_id) {
            let _ = ex.stop.send(());
        }
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardInfo {
    pub id: u32,
    pub local_port: u16,
    pub namespace: String,
    pub pod: String,
    pub pod_port: u16,
}

impl ForwardInfo {
    fn new(id: u32, t: &ForwardTarget) -> Self {
        ForwardInfo {
            id,
            local_port: t.local_port,
            namespace: t.namespace.clone(),
            pod: t.pod.clone(),
            pod_port: t.pod_port,
        }
    }
}

#[tauri::command]
pub async fn start_forward(
    tab_id: u32,
    namespace: String,
    pod: String,
    pod_port: u16,
    sessions: State<'_, Sessions>,
) -> Result<ForwardInfo, String> {
    let client = session_of(&sessions, tab_id).await?.client.clone();
    let (stop_tx, stop_rx) = oneshot::channel();
    let (local_port, _handle) =
        kxs_cluster::portforward::start(client, namespace.clone(), pod.clone(), pod_port, stop_rx)
            .await?;
    let target = ForwardTarget {
        local_port,
        namespace,
        pod,
        pod_port,
    };
    let mut map = sessions.0.lock().await;
    let h = map.get_mut(&tab_id).ok_or("no session for tab")?;
    let id = h.next_forward_id;
    h.next_forward_id += 1;
    let info = ForwardInfo::new(id, &target);
    h.forwards.insert(id, (target, stop_tx));
    Ok(info)
}

#[tauri::command]
pub async fn stop_forward(
    tab_id: u32,
    forward_id: u32,
    sessions: State<'_, Sessions>,
) -> Result<(), String> {
    let mut map = sessions.0.lock().await;
    if let Some(h) = map.get_mut(&tab_id) {
        if let Some((_, stop)) = h.forwards.remove(&forward_id) {
            let _ = stop.send(());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn list_forwards(
    tab_id: u32,
    sessions: State<'_, Sessions>,
) -> Result<Vec<ForwardInfo>, String> {
    let map = sessions.0.lock().await;
    let h = map.get(&tab_id).ok_or("no session for tab")?;
    let mut v: Vec<ForwardInfo> = h
        .forwards
        .iter()
        .map(|(id, (t, _))| ForwardInfo::new(*id, t))
        .collect();
    v.sort_by_key(|f| f.id);
    Ok(v)
}

#[tauri::command]
pub async fn pod_metrics(
    tab_id: u32,
    namespace: Option<String>,
    sessions: State<'_, Sessions>,
) -> Result<Vec<kxs_cluster::metrics::MetricsRow>, String> {
    let client = session_of(&sessions, tab_id).await?.client.clone();
    kxs_cluster::metrics::pod_metrics(client, namespace.as_deref()).await
}
