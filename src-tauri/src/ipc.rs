use kxs_core::kubeconfig::spec::{apply_context_spec, ContextSpec};
use kxs_core::kubeconfig::store::KubeconfigStore;
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

pub struct AppState {
    pub store: Mutex<KubeconfigStore>,
    pub warnings: Mutex<Vec<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KubeconfigView {
    pub contexts: Vec<ContextSummaryDto>,
    pub current_context: Option<String>,
    pub files: Vec<String>,
    pub default_target: String,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSummaryDto {
    pub name: String,
    pub cluster: String,
    pub user: String,
    pub namespace: Option<String>,
    pub source: String,
}

pub fn view(store: &KubeconfigStore, warnings: &[String]) -> KubeconfigView {
    KubeconfigView {
        contexts: store
            .contexts()
            .into_iter()
            .map(|c| ContextSummaryDto {
                name: c.name,
                cluster: c.cluster,
                user: c.user,
                namespace: c.namespace,
                source: c.source.display().to_string(),
            })
            .collect(),
        current_context: store.current_context(),
        files: store
            .paths()
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        default_target: store
            .default_target()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        warnings: warnings.to_vec(),
    }
}

#[tauri::command]
pub fn list_contexts(state: State<AppState>) -> Result<KubeconfigView, String> {
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    let warnings = state.warnings.lock().unwrap_or_else(|e| e.into_inner());
    Ok(view(&store, &warnings))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextDetailDto {
    pub name: String,
    pub namespace: Option<String>,
    pub source: String,
    pub cluster_name: String,
    pub server: Option<String>,
    pub ca_file: Option<String>,
    pub ca_data: Option<String>,
    pub insecure_skip_tls_verify: bool,
    pub user_name: String,
    pub token: Option<String>,
    pub client_certificate: Option<String>,
    pub client_key: Option<String>,
    pub client_certificate_data: Option<String>,
    pub client_key_data: Option<String>,
    pub exec_command: Option<String>,
    pub exec_args: Vec<String>,
    pub exec_env: Vec<[String; 2]>,
    pub exec_api_version: Option<String>,
}

#[tauri::command]
pub fn get_context(name: String, state: State<AppState>) -> Result<ContextDetailDto, String> {
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    let (path, nc) = store
        .find_context(&name)
        .ok_or_else(|| format!("context \"{name}\" not found"))?;
    let cluster = store
        .find_cluster(&nc.context.cluster)
        .map(|(_, c)| c.cluster.clone())
        .unwrap_or_default();
    let user = store
        .find_user(&nc.context.user)
        .map(|(_, u)| u.user.clone())
        .unwrap_or_default();
    Ok(ContextDetailDto {
        name: nc.name.clone(),
        namespace: nc.context.namespace.clone(),
        source: path.display().to_string(),
        cluster_name: nc.context.cluster.clone(),
        server: cluster.server,
        ca_file: cluster.certificate_authority,
        ca_data: cluster.certificate_authority_data,
        insecure_skip_tls_verify: cluster.insecure_skip_tls_verify.unwrap_or(false),
        user_name: nc.context.user.clone(),
        token: user.token,
        client_certificate: user.client_certificate,
        client_key: user.client_key,
        client_certificate_data: user.client_certificate_data,
        client_key_data: user.client_key_data,
        exec_command: user.exec.as_ref().map(|e| e.command.clone()),
        exec_args: user
            .exec
            .as_ref()
            .map(|e| e.args.clone())
            .unwrap_or_default(),
        exec_env: user
            .exec
            .as_ref()
            .and_then(|e| e.env.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|e| [e.name, e.value])
            .collect(),
        exec_api_version: user.exec.as_ref().map(|e| e.api_version.clone()),
    })
}

#[tauri::command]
pub fn save_context(
    spec: ContextSpec,
    app: AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    let mut store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    apply_context_spec(&mut store, spec).map_err(|e| e.to_string())?;
    let _ = app.emit("kubeconfig://changed", ());
    Ok(())
}

#[tauri::command]
pub fn delete_context(name: String, app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let mut store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    store.delete_context(&name).map_err(|e| e.to_string())?;
    let _ = app.emit("kubeconfig://changed", ());
    Ok(())
}
