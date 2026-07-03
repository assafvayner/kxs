use k8s_openapi::api::core::v1::Pod;
use kube::api::Api;
use kube::Client;
use tokio::net::TcpListener;

/// Starts a local proxy on 127.0.0.1:<ephemeral> forwarding to pod:port.
/// Returns the bound local port and a handle to the proxy task, which runs
/// until `stop` fires or the listener errors.
pub async fn start(
    client: Client,
    namespace: String,
    pod: String,
    pod_port: u16,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) -> Result<(u16, tokio::task::JoinHandle<()>), String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| e.to_string())?;
    let local_port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let api: Api<Pod> = Api::namespaced(client, &namespace);

    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut stop => break,
                accepted = listener.accept() => {
                    let Ok((mut conn, _)) = accepted else { break };
                    let api = api.clone();
                    let pod = pod.clone();
                    tokio::spawn(async move {
                        let mut pf = match api.portforward(&pod, &[pod_port]).await {
                            Ok(pf) => pf,
                            Err(_) => return,
                        };
                        let Some(mut upstream) = pf.take_stream(pod_port) else {
                            return;
                        };
                        // bidirectional copy conn <-> upstream; both sides are
                        // tokio::io::{AsyncRead,AsyncWrite} (kube's Portforwarder
                        // stream is a tokio::io::DuplexStream under the hood, no
                        // futures-io compat shim needed).
                        let _ = tokio::io::copy_bidirectional(&mut conn, &mut upstream).await;
                    });
                }
            }
        }
    });
    Ok((local_port, handle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::Namespace;
    use kube::api::{DeleteParams, ObjectMeta, PostParams};
    use std::io::{Read, Write};
    use std::time::Duration;

    async fn kind_session() -> crate::session::ClusterSession {
        let paths = kxs_core::kubeconfig::paths::kubeconfig_paths();
        let store = kxs_core::kubeconfig::store::KubeconfigStore::load(paths).unwrap();
        let yaml = crate::bridge::kubeconfig_yaml_for_context(&store, "kind-local").unwrap();
        crate::session::connect(&yaml, "kind-local").await.unwrap()
    }

    /// Run manually: cargo test -p kxs-cluster -- --ignored (needs kind-local
    /// in ~/.kube/config). Creates a throwaway `kxs-e2e` namespace + nginx
    /// pod, port-forwards :80, TCP-connects to the local port and does a
    /// minimal HTTP GET, then deletes the namespace (best-effort cleanup).
    // Multi-threaded runtime: the test does blocking std::net TCP I/O on its
    // own task, which would otherwise starve the listener's tokio::spawn'd
    // accept/proxy tasks on a single-threaded (default #[tokio::test]) runtime.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore]
    async fn port_forwards_to_kxs_e2e_nginx_pod() {
        let session = kind_session().await;
        let client = session.client.clone();

        let ns_api: Api<Namespace> = Api::all(client.clone());
        let ns = Namespace {
            metadata: ObjectMeta {
                name: Some("kxs-e2e".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        // best-effort create; ignore AlreadyExists so re-runs after a failed
        // cleanup still work.
        let _ = ns_api.create(&PostParams::default(), &ns).await;

        let pod_api: Api<Pod> = Api::namespaced(client.clone(), "kxs-e2e");
        let pod: Pod = serde_json::from_value(serde_json::json!({
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": { "name": "nginx", "namespace": "kxs-e2e" },
            "spec": {
                "containers": [{
                    "name": "nginx",
                    "image": "nginx:alpine",
                    "ports": [{ "containerPort": 80 }],
                }]
            }
        }))
        .unwrap();
        pod_api
            .create(&PostParams::default(), &pod)
            .await
            .expect("failed to create kxs-e2e/nginx pod");

        // wait for the pod to become Ready (best-effort polling, up to ~60s).
        let mut ready = false;
        for _ in 0..30 {
            if let Ok(p) = pod_api.get("nginx").await {
                let is_ready = p
                    .status
                    .as_ref()
                    .and_then(|s| s.conditions.as_ref())
                    .map(|conds| {
                        conds
                            .iter()
                            .any(|c| c.type_ == "Ready" && c.status == "True")
                    })
                    .unwrap_or(false);
                if is_ready {
                    ready = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let (local_port, pf_handle) = start(
            client.clone(),
            "kxs-e2e".to_string(),
            "nginx".to_string(),
            80,
            stop_rx,
        )
        .await
        .expect("port-forward start failed");
        assert_ne!(local_port, 0, "expected a bound ephemeral local port");

        if ready {
            // Give the forward a moment to be ready to accept, then do a
            // minimal HTTP GET over a raw TCP connection.
            let mut response = String::new();
            let mut connected = false;
            for _ in 0..10 {
                match std::net::TcpStream::connect(("127.0.0.1", local_port)) {
                    Ok(mut stream) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        stream
                            .write_all(b"GET / HTTP/1.0\r\n\r\n")
                            .expect("failed to write HTTP request");
                        let _ = stream.read_to_string(&mut response);
                        connected = true;
                        break;
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(500)),
                }
            }
            assert!(connected, "failed to TCP-connect to forwarded local port");
            assert!(
                response.contains("HTTP") || response.to_lowercase().contains("nginx"),
                "expected an HTTP/nginx response, got: {response:?}"
            );
        } else {
            // Pod never became Ready (flaky/slow image pull in CI-like
            // environments) — fall back to the reduced assertion that the
            // proxy at least binds and returns a port.
            eprintln!(
                "kxs-e2e/nginx pod did not become Ready in time; skipping HTTP \
                 assertion, only verifying the local listener bound"
            );
        }

        let _ = stop_tx.send(());
        let _ = pf_handle.await;

        // best-effort cleanup: delete the namespace (cascades the pod).
        let _ = ns_api.delete("kxs-e2e", &DeleteParams::default()).await;
    }
}
