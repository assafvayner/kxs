//! Mutations and terminal-suspend actions, dispatched to kxs-cluster.

use k8s_openapi::chrono::Utc;
use kxs_cluster::edit::{
    apply_edit, cordon_patch, delete_resource, merge_patch, restart_patch, scale_patch,
    suspend_patch,
};
use ratatui::crossterm;

use crate::cmd::Mutation;
use kxs_cluster::discovery::ResourceKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Dispatch a mutation to its kxs-cluster call. `Ok(Some(text))` surfaces a
/// flash message (e.g. the created Job name).
pub async fn mutate(client: kube::Client, m: Mutation) -> Result<Option<String>, String> {
    match m {
        Mutation::Scale {
            kind,
            ns,
            name,
            replicas,
        } => {
            merge_patch(
                client,
                &kind.group,
                &kind.version,
                &kind.kind,
                &kind.plural,
                Some(&ns),
                &name,
                scale_patch(replicas),
            )
            .await?;
            Ok(Some(format!("scaled {name} to {replicas}")))
        }
        Mutation::Restart { kind, ns, name } => {
            let now = Utc::now().to_rfc3339();
            merge_patch(
                client,
                &kind.group,
                &kind.version,
                &kind.kind,
                &kind.plural,
                Some(&ns),
                &name,
                restart_patch(&now),
            )
            .await?;
            Ok(Some(format!("restart rolled out for {name}")))
        }
        Mutation::Cordon {
            name,
            unschedulable,
            ..
        } => {
            merge_patch(
                client,
                "",
                "v1",
                "Node",
                "nodes",
                None,
                &name,
                cordon_patch(unschedulable),
            )
            .await?;
            Ok(Some(if unschedulable {
                format!("cordoned {name}")
            } else {
                format!("uncordoned {name}")
            }))
        }
        Mutation::Drain { name, .. } => {
            let report = kxs_cluster::workloads::drain_node(client, &name).await?;
            Ok(Some(format!(
                "drained {name}: evicted {} skipped {} failed {}",
                report.evicted,
                report.skipped,
                report.failed.len()
            )))
        }
        Mutation::Trigger { ns, name } => {
            let job = kxs_cluster::workloads::trigger_cronjob(client, &ns, &name).await?;
            Ok(Some(format!("created job {job}")))
        }
        Mutation::Suspend { ns, name, suspend } => {
            merge_patch(
                client,
                "batch",
                "v1",
                "CronJob",
                "cronjobs",
                Some(&ns),
                &name,
                suspend_patch(suspend),
            )
            .await?;
            Ok(Some(if suspend {
                format!("suspended {name}")
            } else {
                format!("resumed {name}")
            }))
        }
        Mutation::Undo { ns, name, revision } => {
            kxs_cluster::workloads::rollout_undo(client, &ns, &name, revision).await?;
            Ok(Some(format!("rolled back {name} to revision {revision}")))
        }
        Mutation::Delete {
            kind,
            ns,
            name,
            propagation,
            force,
        } => {
            let ns = if kind.namespaced {
                Some(ns.as_str())
            } else {
                None
            };
            delete_resource(
                client,
                &kind.group,
                &kind.version,
                &kind.kind,
                &kind.plural,
                ns,
                &name,
                propagation.as_deref(),
                force,
            )
            .await?;
            Ok(Some(format!("deleted {}/{}", kind.kind, name)))
        }
    }
}

/// The `s` exec flow, run with the TUI suspended: the remote shell gets the
/// real terminal in raw mode; output is decoded from base64; Ctrl-D / exit
/// restores the TUI. `Ok(Some(text))` is the session's close message.
pub async fn run_exec(
    client: kube::Client,
    ns: &str,
    pod: &str,
    container: Option<&str>,
    cols: u16,
    rows: u16,
) -> Result<Option<String>, String> {
    let shell = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "command -v bash >/dev/null && exec bash || exec sh".to_string(),
    ];
    run_session(client, ns, pod, container, cols, rows, Some(shell)).await
}

/// The `a` attach flow: join the container's already-running process instead
/// of starting a shell. Same terminal handoff as `run_exec`.
pub async fn run_attach(
    client: kube::Client,
    ns: &str,
    pod: &str,
    container: Option<&str>,
    cols: u16,
    rows: u16,
) -> Result<Option<String>, String> {
    run_session(client, ns, pod, container, cols, rows, None).await
}

/// Drives one interactive remote session on the real terminal.
/// `command` = `Some(argv)` execs it, `None` attaches to the running process.
async fn run_session(
    client: kube::Client,
    ns: &str,
    pod: &str,
    container: Option<&str>,
    cols: u16,
    rows: u16,
    command: Option<Vec<String>>,
) -> Result<Option<String>, String> {
    use base64::Engine;
    use std::io::Write;

    // re-raw the terminal ourselves: the remote shell needs raw input
    crossterm::terminal::enable_raw_mode().map_err(|e| e.to_string())?;
    let (close_tx, mut close_rx) = tokio::sync::mpsc::unbounded_channel::<Option<String>>();
    let send = move |ev: kxs_cluster::exec::ExecEvent| match ev {
        kxs_cluster::exec::ExecEvent::Output { data } => {
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&data) {
                let mut out = std::io::stdout();
                let _ = out.write_all(&bytes);
                let _ = out.flush();
            }
            true
        }
        kxs_cluster::exec::ExecEvent::Closed { message } => {
            let _ = close_tx.send(message);
            false
        }
    };
    let opened = match command {
        Some(command) => {
            kxs_cluster::exec::exec(client, ns, pod, container, command, cols, rows, send).await
        }
        None => kxs_cluster::exec::attach(client, ns, pod, container, cols, rows, send).await,
    };
    let handle = match opened {
        Ok(h) => h,
        // do not leave raw mode enabled on the error path
        Err(e) => {
            crossterm::terminal::disable_raw_mode().ok();
            return Err(e);
        }
    };
    // stdin pump: raw bytes from the real terminal. poll()-based with a
    // stop flag so it can actually exit when the session ends — a plain
    // blocking read would linger, eat the next keys whole, and hold the tty
    // in a blocked read across the terminal-mode switches that follow.
    let stdin_tx = handle.stdin.clone();
    let stdin_stop = Arc::new(AtomicBool::new(false));
    let stdin_done = Arc::new(AtomicBool::new(false));
    {
        let stop = stdin_stop.clone();
        let done = stdin_done.clone();
        std::thread::spawn(move || {
            use std::io::Read as _;
            let mut buf = [0u8; 1024];
            let mut input = std::io::stdin();
            while !stop.load(Ordering::SeqCst) {
                let mut fds = [libc::pollfd {
                    fd: 0,
                    events: libc::POLLIN,
                    revents: 0,
                }];
                let rc = unsafe { libc::poll(fds.as_mut_ptr(), 1, 100) };
                if rc <= 0 {
                    continue;
                }
                match input.read(&mut buf) {
                    Ok(0) => break, // local EOF
                    Ok(n) => {
                        if stdin_tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            done.store(true, Ordering::SeqCst);
        });
    }

    // SIGWINCH-ish: poll the terminal size and resize the remote pty
    let resize_tx = handle.resize.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if let Ok((w, h)) = crossterm::terminal::size() {
                if resize_tx.send((w, h)).is_err() {
                    break;
                }
            }
        }
    });

    let message = close_rx.recv().await.unwrap_or(None);
    // stop the stdin pump and wait until it is out of read(2), so it can
    // neither eat the TUI's next keys nor race the terminal-mode switches
    stdin_stop.store(true, Ordering::SeqCst);
    for _ in 0..50 {
        if stdin_done.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // restore: leave raw mode (the TUI re-enters via terminal::enter)
    crossterm::terminal::disable_raw_mode().ok();
    Ok(message)
}

/// The `e` edit flow, run with the TUI suspended: fetch YAML, write a temp
/// file, hand it to the configured editor (else $KUBE_EDITOR/$EDITOR/vi),
/// apply on save. Server errors are
/// prepended as `# ` comments and the editor reopens (kubectl behavior).
/// `Ok(None)` means "no changes".
pub async fn run_edit(
    client: kube::Client,
    kind: ResourceKind,
    ns: Option<String>,
    name: String,
    editor: Option<String>,
) -> Result<Option<String>, String> {
    let yaml = kxs_cluster::resources::get_yaml(
        client.clone(),
        &kind.group,
        &kind.version,
        &kind.kind,
        &kind.plural,
        ns.as_deref(),
        &name,
    )
    .await?;
    // NamedTempFile creates the file atomically with restrictive permissions
    // and removes it on every return path, including editor/read failures.
    let mut file = tempfile::Builder::new()
        .prefix(&format!("kxs-{}-{}-", kind.plural, name))
        .suffix(".yaml")
        .tempfile()
        .map_err(|e| e.to_string())?;
    use std::os::unix::fs::PermissionsExt;
    file.as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    let path = file.path().to_path_buf();
    let mut current = yaml.clone();
    let editor = editor
        .filter(|e| !e.trim().is_empty())
        .or_else(|| std::env::var("KUBE_EDITOR").ok())
        .or_else(|| std::env::var("EDITOR").ok())
        .unwrap_or_else(|| "vi".into());
    let outcome = loop {
        write_secret_file(&mut file, &current)?;
        // shell form so editors with arguments ("code -w") work
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("{editor} \"$0\""))
            .arg(&path)
            .status()
            .map_err(|e| format!("cannot run editor {editor}: {e}"))?;
        if !status.success() {
            return Err(format!("editor exited with {status}; aborting edit"));
        }
        let edited = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        if edited == current {
            break Ok(None); // unchanged: abort
        }
        let result = apply_edit(
            client.clone(),
            &kind.group,
            &kind.version,
            &kind.kind,
            &kind.plural,
            ns.as_deref(),
            &name,
            &yaml,
            &edited,
            false,
        )
        .await;
        match result {
            Ok(_) => break Ok(Some(format!("edited {}/{}", kind.kind, name))),
            Err(e) => {
                // prepend the error as a comment and reopen the editor
                current = format!(
                    "# edit failed, fix or save unchanged to abort:\n# {}\n\n{}",
                    e.replace('\n', "\n# "),
                    edited
                );
            }
        }
    };
    outcome
}

fn write_secret_file(file: &mut tempfile::NamedTempFile, content: &str) -> Result<(), String> {
    use std::io::{Seek, Write};
    let f = file.as_file_mut();
    f.set_len(0).map_err(|e| e.to_string())?;
    f.rewind().map_err(|e| e.to_string())?;
    f.write_all(content.as_bytes()).map_err(|e| e.to_string())?;
    f.flush().map_err(|e| e.to_string())
}
