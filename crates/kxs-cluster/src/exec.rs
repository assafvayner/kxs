use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use futures::SinkExt;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, AttachParams, AttachedProcess, TerminalSize};
use kube::Client;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExecEvent {
    /// terminal output (stdout+stderr merged), base64 so binary/ANSI survives IPC
    Output {
        data: String,
    },
    Closed {
        message: Option<String>,
    },
}

/// Handle for an attached exec session: write to stdin, request resize, or stop.
pub struct ExecHandle {
    pub stdin: mpsc::UnboundedSender<Vec<u8>>,
    pub resize: mpsc::UnboundedSender<(u16, u16)>,
    pub stop: tokio::sync::oneshot::Sender<()>,
}

/// Attach `command` (e.g. ["/bin/sh"]) in `pod`/`container`, streaming merged
/// output to `send` (base64). Returns a handle for stdin/resize/stop.
#[allow(clippy::too_many_arguments)]
pub async fn exec(
    client: Client,
    namespace: &str,
    pod: &str,
    container: Option<&str>,
    command: Vec<String>,
    cols: u16,
    rows: u16,
    send: impl Fn(ExecEvent) -> bool + Send + Sync + 'static,
) -> Result<ExecHandle, String> {
    let api: Api<Pod> = Api::namespaced(client, namespace);
    let mut ap = AttachParams::interactive_tty();
    if let Some(c) = container {
        ap = ap.container(c.to_string());
    }
    let mut proc: AttachedProcess = api
        .exec(pod, command, &ap)
        .await
        .map_err(|e| e.to_string())?;

    let mut out = proc.stdout().ok_or("no stdout from exec")?;
    let mut stdin_writer = proc.stdin().ok_or("no stdin from exec")?;
    let mut resize_writer = proc.terminal_size();
    let status_fut = proc.take_status().ok_or("exec status already taken")?;

    let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (resize_tx, mut resize_rx) = mpsc::unbounded_channel::<(u16, u16)>();
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();

    // initial size
    if let Some(rw) = resize_writer.as_mut() {
        let _ = rw
            .send(TerminalSize {
                width: cols,
                height: rows,
            })
            .await;
    }

    // Signalled by the stdout pump when the output sink is dead, so the
    // control loop can tear the session down deterministically instead of
    // parking until the next stdout write or an explicit stop.
    let sink_closed = std::sync::Arc::new(tokio::sync::Notify::new());

    // stdout pump -> send (base64)
    let send = std::sync::Arc::new(send);
    let send_out = send.clone();
    let sink_closed_pump = sink_closed.clone();
    tokio::spawn(async move {
        let mut buf = [0u8; 8192];
        loop {
            match out.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let data = BASE64.encode(&buf[..n]);
                    if !send_out(ExecEvent::Output { data }) {
                        sink_closed_pump.notify_one();
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // control loop: stdin, resize, stop, and completion (status taken exactly once above)
    let send_end = send.clone();
    let sink_closed_ctrl = sink_closed.clone();
    tokio::spawn(async move {
        tokio::pin!(status_fut);
        let mut stdin_open = true;
        let mut resize_open = resize_writer.is_some();
        loop {
            tokio::select! {
                _ = &mut stop_rx => {
                    // Abort the underlying attach task so the connection to
                    // the kubelet actually tears down instead of leaking.
                    proc.abort();
                    break;
                }
                _ = sink_closed_ctrl.notified() => {
                    // Output sink is dead; abort the attach task rather than
                    // leaking the websocket until the next stdout write.
                    proc.abort();
                    let _ = send_end(ExecEvent::Closed { message: None });
                    break;
                }
                msg = stdin_rx.recv(), if stdin_open => {
                    match msg {
                        Some(bytes) => {
                            if stdin_writer.write_all(&bytes).await.is_err() {
                                break;
                            }
                            let _ = stdin_writer.flush().await;
                        }
                        None => stdin_open = false,
                    }
                }
                msg = resize_rx.recv(), if resize_open => {
                    match msg {
                        Some((c, r)) => {
                            if let Some(rw) = resize_writer.as_mut() {
                                let _ = rw.send(TerminalSize { width: c, height: r }).await;
                            }
                        }
                        None => resize_open = false,
                    }
                }
                status = &mut status_fut => {
                    let msg = status.and_then(|s| s.message.or(s.reason));
                    let _ = send(ExecEvent::Closed { message: msg });
                    break;
                }
            }
        }
    });

    Ok(ExecHandle {
        stdin: stdin_tx,
        resize: resize_tx,
        stop: stop_tx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn kind_session() -> crate::session::ClusterSession {
        let paths = kxs_core::kubeconfig::paths::kubeconfig_paths();
        let store = kxs_core::kubeconfig::store::KubeconfigStore::load(paths).unwrap();
        let yaml = crate::bridge::kubeconfig_yaml_for_context(&store, "kind-local").unwrap();
        crate::session::connect(&yaml, "kind-local").await.unwrap()
    }

    /// Run manually: cargo test -p kxs-cluster -- --ignored (needs kind-local in ~/.kube/config)
    ///
    /// Execs into a kube-proxy pod (DaemonSet, has /bin/sh, read-safe — no
    /// mutation) in kube-system and checks an echoed marker string round-trips
    /// through the base64-encoded Output events.
    #[tokio::test]
    #[ignore]
    async fn execs_echo_into_kind_local_pod() {
        let session = kind_session().await;
        let api: Api<Pod> = Api::namespaced(session.client.clone(), "kube-system");
        let pods = api
            .list(&kube::api::ListParams::default().labels("k8s-app=kube-proxy"))
            .await
            .unwrap();
        let pod = pods
            .items
            .into_iter()
            .next()
            .expect("expected a kube-proxy pod in kube-system");
        let pod_name = pod.metadata.name.unwrap();

        let (tx, mut rx) = mpsc::unbounded_channel::<ExecEvent>();
        let handle = exec(
            session.client.clone(),
            "kube-system",
            &pod_name,
            None,
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "echo kxs-hello".to_string(),
            ],
            80,
            24,
            move |ev| tx.send(ev).is_ok(),
        )
        .await
        .unwrap();

        let mut decoded = String::new();
        loop {
            let ev = tokio::time::timeout(std::time::Duration::from_secs(15), rx.recv())
                .await
                .expect("timed out waiting for exec event")
                .expect("exec channel closed early");
            match ev {
                ExecEvent::Output { data } => {
                    let bytes = BASE64.decode(data).unwrap();
                    decoded.push_str(&String::from_utf8_lossy(&bytes));
                }
                ExecEvent::Closed { .. } => break,
            }
        }

        assert!(
            decoded.contains("kxs-hello"),
            "expected decoded exec output to contain marker, got: {decoded:?}"
        );

        // handle.stop would tear down the attach task; nothing left running
        // here since the remote command already exited and closed the stream.
        drop(handle);
    }
}
