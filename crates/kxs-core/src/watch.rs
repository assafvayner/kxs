//! Kubeconfig file watching, behind the `watch` feature. Shared by both
//! frontends: the desktop app reloads its `AppState` store, the TUI refreshes
//! its Contexts view.

use notify::Watcher;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// Watches the parent dirs of all kubeconfig files; on changes (debounced to
/// 300ms of quiet) calls `on_change` on a background thread. Our own writes
/// and backups also fire this — a redundant refresh is harmless.
pub fn spawn_watcher(paths: Vec<PathBuf>, on_change: impl Fn() + Send + 'static) {
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
            if let Err(e) = watcher.watch(d, notify::RecursiveMode::NonRecursive) {
                eprintln!("kxs: cannot watch {}: {e}", d.display());
            }
        }
        while rx.recv().is_ok() {
            while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}
            on_change();
        }
    });
}
