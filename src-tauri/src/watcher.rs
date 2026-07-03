use notify::{RecursiveMode, Watcher};
use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Watches the parent dirs of all kubeconfig files; on changes (debounced to
/// 300ms of quiet) reloads the store and notifies the frontend. Our own writes
/// and backups also fire this — a redundant refresh is harmless.
pub fn spawn(app: AppHandle, paths: Vec<std::path::PathBuf>) {
    std::thread::spawn(move || {
        let (tx, rx) = mpsc::channel();
        let mut watcher = match notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        }) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("kxs: kubeconfig watcher unavailable: {e}");
                return;
            }
        };
        let mut dirs: Vec<_> = paths
            .iter()
            .filter_map(|p| p.parent().map(|d| d.to_path_buf()))
            .collect();
        dirs.sort();
        dirs.dedup();
        for d in &dirs {
            if let Err(e) = watcher.watch(d, RecursiveMode::NonRecursive) {
                eprintln!("kxs: cannot watch {}: {e}", d.display());
            }
        }
        while rx.recv().is_ok() {
            while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}
            let state = app.state::<crate::ipc::AppState>();
            if let Ok(mut store) = state.store.lock() {
                let fresh_warnings = store.reload();
                if let Ok(mut warnings) = state.warnings.lock() {
                    *warnings = fresh_warnings;
                }
            }
            let _ = app.emit("kubeconfig://changed", ());
        }
    });
}
