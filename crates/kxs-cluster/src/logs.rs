use futures::{AsyncBufReadExt, TryStreamExt};
use k8s_openapi::api::core::v1::Pod;
use kube::api::LogParams;
use kube::{Api, Client};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LogEvent {
    Lines { lines: Vec<String> },
    Error { message: String },
    Eof,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRequest {
    pub namespace: String,
    pub pod: String,
    pub container: Option<String>,
    pub follow: bool,
    pub tail_lines: Option<i64>,
    pub since_seconds: Option<i64>,
    pub timestamps: bool,
}

pub async fn list_containers(
    client: Client,
    namespace: &str,
    pod: &str,
) -> Result<Vec<String>, String> {
    let api: Api<Pod> = Api::namespaced(client, namespace);
    let p = api.get(pod).await.map_err(|e| e.to_string())?;
    let spec = p.spec.ok_or("pod has no spec")?;
    let mut names: Vec<String> = Vec::new();
    if let Some(init) = spec.init_containers {
        names.extend(init.into_iter().map(|c| c.name));
    }
    names.extend(spec.containers.into_iter().map(|c| c.name));
    Ok(names)
}

/// Streams log lines in batches (50 lines or 100ms, whichever first) until
/// EOF, stop, or the receiver goes away.
pub async fn run_log_stream(
    client: Client,
    req: LogRequest,
    send: impl Fn(LogEvent) -> bool + Send + 'static,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) {
    let api: Api<Pod> = Api::namespaced(client, &req.namespace);
    let params = LogParams {
        follow: req.follow,
        container: req.container.clone(),
        tail_lines: req.tail_lines,
        since_seconds: req.since_seconds,
        timestamps: req.timestamps,
        ..Default::default()
    };
    let reader = match api.log_stream(&req.pod, &params).await {
        Ok(r) => r,
        Err(e) => {
            let _ = send(LogEvent::Error {
                message: e.to_string(),
            });
            return;
        }
    };
    let mut lines = reader.lines();
    let mut batch: Vec<String> = Vec::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = &mut stop => return,
            _ = tick.tick() => {
                if !batch.is_empty() && !send(LogEvent::Lines { lines: std::mem::take(&mut batch) }) {
                    return;
                }
            }
            line = lines.try_next() => match line {
                Ok(Some(l)) => {
                    batch.push(l);
                    if batch.len() >= 50 && !send(LogEvent::Lines { lines: std::mem::take(&mut batch) }) {
                        return;
                    }
                }
                Ok(None) => {
                    if !batch.is_empty() {
                        let _ = send(LogEvent::Lines { lines: std::mem::take(&mut batch) });
                    }
                    let _ = send(LogEvent::Eof);
                    return;
                }
                Err(e) => {
                    let _ = send(LogEvent::Error { message: e.to_string() });
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run manually: cargo test -p kxs-cluster -- --ignored (needs kind-local in ~/.kube/config)
    #[tokio::test]
    #[ignore]
    async fn streams_logs_from_kind_local() {
        let paths = kxs_core::kubeconfig::paths::kubeconfig_paths();
        let store = kxs_core::kubeconfig::store::KubeconfigStore::load(paths).unwrap();
        let yaml = crate::bridge::kubeconfig_yaml_for_context(&store, "kind-local").unwrap();
        let session = crate::session::connect(&yaml, "kind-local").await.unwrap();
        let api: Api<Pod> = Api::namespaced(session.client.clone(), "kube-system");
        let pod = api
            .list(&kube::api::ListParams::default().limit(1))
            .await
            .unwrap()
            .items
            .into_iter()
            .next()
            .unwrap();
        let req = LogRequest {
            namespace: "kube-system".into(),
            pod: pod.metadata.name.unwrap(),
            container: None,
            follow: false,
            tail_lines: Some(5),
            since_seconds: None,
            timestamps: false,
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (_stop, stop_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(run_log_stream(
            session.client.clone(),
            req,
            move |e| tx.send(e).is_ok(),
            stop_rx,
        ));
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(15), rx.recv())
                .await
                .unwrap()
                .unwrap();
            match ev {
                LogEvent::Eof => break,
                LogEvent::Error { message } => panic!("log stream error: {message}"),
                LogEvent::Lines { .. } => continue,
            }
        }
    }
}
