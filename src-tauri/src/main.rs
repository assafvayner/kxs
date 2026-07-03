#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cluster_ipc;
mod ipc;
mod watcher;

use kxs_core::kubeconfig::paths::kubeconfig_paths;
use kxs_core::kubeconfig::store::KubeconfigStore;

fn main() {
    let paths = kubeconfig_paths();
    let (store, warnings) = KubeconfigStore::load_tolerant(paths.clone());
    for w in &warnings {
        eprintln!("kxs: {w}");
    }
    tauri::Builder::default()
        .manage(ipc::AppState {
            store: std::sync::Mutex::new(store),
            warnings: std::sync::Mutex::new(warnings),
        })
        .manage(cluster_ipc::Sessions::default())
        .invoke_handler(tauri::generate_handler![
            ipc::list_contexts,
            ipc::get_context,
            ipc::save_context,
            ipc::delete_context,
            ipc::ping_context,
            cluster_ipc::open_session,
            cluster_ipc::close_session,
            cluster_ipc::list_namespaces,
            cluster_ipc::watch_pods,
            cluster_ipc::list_containers,
            cluster_ipc::stream_logs,
            cluster_ipc::stop_logs
        ])
        .setup(move |app| {
            watcher::spawn(app.handle().clone(), paths);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running kxs");
}
