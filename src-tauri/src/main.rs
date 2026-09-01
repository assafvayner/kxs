#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cluster_ipc;
mod ipc;
mod shell_env;
mod watcher;

use kxs_core::kubeconfig::paths::kubeconfig_paths;
use kxs_core::kubeconfig::store::KubeconfigStore;

fn main() {
    // Restore the user's real PATH before anything spawns exec-auth helpers
    // (EKS `aws eks get-token`, gke-gcloud-auth-plugin, kubelogin, ...).
    shell_env::augment_path();

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
            cluster_ipc::stop_logs,
            cluster_ipc::list_resource_kinds,
            cluster_ipc::list_present_kinds,
            cluster_ipc::list_resource_table,
            cluster_ipc::watch_resource_table,
            cluster_ipc::stop_resource_table,
            cluster_ipc::get_resource_yaml,
            cluster_ipc::get_resource_events,
            cluster_ipc::apply_resource_yaml,
            cluster_ipc::delete_resource,
            cluster_ipc::scale_resource,
            cluster_ipc::restart_resource,
            cluster_ipc::cordon_node,
            cluster_ipc::start_exec,
            cluster_ipc::exec_stdin,
            cluster_ipc::exec_resize,
            cluster_ipc::stop_exec,
            cluster_ipc::start_forward,
            cluster_ipc::forward_service,
            cluster_ipc::stop_forward,
            cluster_ipc::list_forwards,
            cluster_ipc::list_workload_pods,
            cluster_ipc::rollout_history,
            cluster_ipc::rollout_undo,
            cluster_ipc::drain_node,
            cluster_ipc::trigger_cronjob,
            cluster_ipc::suspend_cronjob,
            cluster_ipc::get_config_values,
            cluster_ipc::pod_metrics
        ])
        .setup(move |app| {
            watcher::spawn(app.handle().clone(), paths);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running kxs");
}
