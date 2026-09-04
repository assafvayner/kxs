//! Kubeconfig file watching, behind the `watch` feature. Shared by both
//! frontends: the desktop app reloads its `AppState` store, the TUI refreshes
//! its Contexts view.

use notify::Watcher;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// True when an event touches one of the kubeconfig files (or carries no
/// paths at all). Editors rename over the file, so a Create/Rename event on
/// the same path counts.
fn is_relevant(files: &[PathBuf], event_paths: &[PathBuf]) -> bool {
    event_paths.is_empty()
        || event_paths.iter().any(|p| {
            files.iter().any(|f| {
                f == p
                    || std::fs::canonicalize(f)
                        .ok()
                        .is_some_and(|cf| cf == *p || std::fs::canonicalize(p).ok() == Some(cf))
            })
        })
}

/// Watches the parent dirs of all kubeconfig files; on changes (debounced to
/// 300ms of quiet) calls `on_change` on a background thread. Our own writes
/// and backups also fire this — a redundant refresh is harmless.
pub fn spawn_watcher(paths: Vec<PathBuf>, on_change: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let (tx, rx) = mpsc::channel();
        let files: Vec<PathBuf> = paths.clone();
        let mut watcher =
            match notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                match res {
                    Ok(ev) if !is_relevant(&files, &ev.paths) => {}
                    other => {
                        let _ = tx.send(other);
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_watched_files_pass() {
        let files = vec![PathBuf::from("/home/u/.kube/config")];
        assert!(is_relevant(
            &files,
            &[PathBuf::from("/home/u/.kube/config")]
        ));
        assert!(!is_relevant(
            &files,
            &[PathBuf::from("/home/u/.kube/cache/x")]
        ));
        assert!(
            is_relevant(&files, &[]),
            "events without paths are not filtered"
        );
    }
}
