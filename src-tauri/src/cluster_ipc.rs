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
pub struct Sessions(pub Mutex<HashMap<u32, SessionHandle>>);

pub struct SessionHandle {
    pub session: Arc<ClusterSession>,
    pub pod_stop: Option<oneshot::Sender<()>>,
}

impl SessionHandle {
    fn stop_all(self) {
        if let Some(stop) = self.pod_stop {
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
