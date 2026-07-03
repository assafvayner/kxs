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

pub struct SessionHandle {
    pub session: Arc<ClusterSession>,
    pub pod_stop: Option<oneshot::Sender<()>>,
    pub log_stops: HashMap<u32, oneshot::Sender<()>>,
    pub next_log_id: u32,
}

impl SessionHandle {
    fn stop_all(self) {
        if let Some(stop) = self.pod_stop {
            let _ = stop.send(());
        }
        for (_, stop) in self.log_stops {
            let _ = stop.send(());
        }
    }
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
            next_log_id: 0,
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
        let id = handle.next_log_id;
        handle.next_log_id += 1;
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
    name: String,
    sessions: State<'_, Sessions>,
) -> Result<Vec<kxs_cluster::resources::ResourceEvent>, String> {
    let session = session_of(&sessions, tab_id).await?;
    Ok(
        kxs_cluster::resources::get_events(session.client.clone(), namespace.as_deref(), &name)
            .await,
    )
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
