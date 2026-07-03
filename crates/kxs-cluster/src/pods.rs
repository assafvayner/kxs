use k8s_openapi::api::core::v1::Pod;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PodRow {
    pub key: String,
    pub name: String,
    pub namespace: String,
    pub ready: String,
    pub status: String,
    pub restarts: u32,
    pub ip: Option<String>,
    pub node: Option<String>,
    /// RFC3339; age is rendered client-side so it can tick.
    pub created: Option<String>,
}

pub fn pod_key(pod: &Pod) -> String {
    format!(
        "{}/{}",
        pod.metadata.namespace.as_deref().unwrap_or_default(),
        pod.metadata.name.as_deref().unwrap_or_default()
    )
}

/// kubectl-printer-style status summary (simplified but covering the common
/// states: waiting reasons, init progress, Terminating, Completed, Evicted).
pub fn pod_row(pod: &Pod) -> PodRow {
    let meta = &pod.metadata;
    let name = meta.name.clone().unwrap_or_default();
    let namespace = meta.namespace.clone().unwrap_or_default();
    let status = pod.status.as_ref();
    let container_statuses = status
        .and_then(|s| s.container_statuses.as_deref())
        .unwrap_or(&[]);
    let total = pod.spec.as_ref().map(|s| s.containers.len()).unwrap_or(0);
    let ready_count = container_statuses.iter().filter(|c| c.ready).count();
    let restarts: u32 = container_statuses
        .iter()
        .map(|c| c.restart_count as u32)
        .sum();

    let phase = status
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "Unknown".into());
    let mut display = status
        .and_then(|s| s.reason.clone())
        .unwrap_or(phase.clone());
    if phase == "Succeeded" && display == "Succeeded" {
        display = "Completed".into();
    }

    if let Some(ics) = status.and_then(|s| s.init_container_statuses.as_deref()) {
        let total_init = pod
            .spec
            .as_ref()
            .and_then(|s| s.init_containers.as_ref())
            .map(|v| v.len())
            .unwrap_or(ics.len());
        let mut done = 0usize;
        for ic in ics {
            let state = ic.state.as_ref();
            if let Some(t) = state.and_then(|s| s.terminated.as_ref()) {
                if t.exit_code == 0 {
                    done += 1;
                    continue;
                }
                display = format!(
                    "Init:{}",
                    t.reason
                        .clone()
                        .unwrap_or_else(|| format!("ExitCode:{}", t.exit_code))
                );
                done = usize::MAX;
                break;
            }
            if let Some(w) = state.and_then(|s| s.waiting.as_ref()) {
                if let Some(r) = w.reason.as_deref() {
                    if r != "PodInitializing" {
                        display = format!("Init:{r}");
                        done = usize::MAX;
                        break;
                    }
                }
            }
            display = format!("Init:{done}/{total_init}");
            done = usize::MAX;
            break;
        }
        if done != usize::MAX && done < total_init {
            display = format!("Init:{done}/{total_init}");
        }
    }

    if !display.starts_with("Init:") {
        for c in container_statuses {
            if let Some(w) = c.state.as_ref().and_then(|s| s.waiting.as_ref()) {
                if let Some(r) = w.reason.clone() {
                    display = r;
                    break;
                }
            }
        }
    }

    if meta.deletion_timestamp.is_some() {
        display = "Terminating".into();
    }

    PodRow {
        key: format!("{namespace}/{name}"),
        name,
        namespace,
        ready: format!("{ready_count}/{total}"),
        status: display,
        restarts,
        ip: status.and_then(|s| s.pod_ip.clone()),
        node: pod.spec.as_ref().and_then(|s| s.node_name.clone()),
        created: meta.creation_timestamp.as_ref().map(|t| t.0.to_rfc3339()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::Pod;

    fn pod(v: serde_json::Value) -> Pod {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn running_pod() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "web-1", "namespace": "app", "creationTimestamp": "2026-07-01T00:00:00Z"},
            "spec": {"containers": [{"name": "a"}, {"name": "b"}], "nodeName": "node-1"},
            "status": {"phase": "Running", "podIP": "10.0.0.5", "containerStatuses": [
                {"name": "a", "ready": true, "restartCount": 2, "state": {"running": {}}},
                {"name": "b", "ready": false, "restartCount": 0, "state": {"running": {}}}
            ]}
        }));
        let r = pod_row(&p);
        assert_eq!(r.key, "app/web-1");
        assert_eq!(r.ready, "1/2");
        assert_eq!(r.status, "Running");
        assert_eq!(r.restarts, 2);
        assert_eq!(r.ip.as_deref(), Some("10.0.0.5"));
        assert_eq!(r.node.as_deref(), Some("node-1"));
        assert_eq!(r.created.as_deref(), Some("2026-07-01T00:00:00+00:00"));
    }

    #[test]
    fn waiting_reason_wins() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [{"name": "a"}]},
            "status": {"phase": "Running", "containerStatuses": [
                {"name": "a", "ready": false, "restartCount": 14,
                 "state": {"waiting": {"reason": "CrashLoopBackOff"}}}
            ]}
        }));
        assert_eq!(pod_row(&p).status, "CrashLoopBackOff");
    }

    #[test]
    fn init_containers_take_precedence() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [{"name": "a"}], "initContainers": [{"name": "i0"}, {"name": "i1"}]},
            "status": {"phase": "Pending", "initContainerStatuses": [
                {"name": "i0", "ready": false, "restartCount": 0, "state": {"running": {}}},
                {"name": "i1", "ready": false, "restartCount": 0, "state": {"waiting": {"reason": "PodInitializing"}}}
            ]}
        }));
        assert_eq!(pod_row(&p).status, "Init:0/2");
    }

    #[test]
    fn init_crash_reason_shown() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [{"name": "a"}], "initContainers": [{"name": "i0"}]},
            "status": {"phase": "Pending", "initContainerStatuses": [
                {"name": "i0", "ready": false, "restartCount": 3,
                 "state": {"waiting": {"reason": "CrashLoopBackOff"}}}
            ]}
        }));
        assert_eq!(pod_row(&p).status, "Init:CrashLoopBackOff");
    }

    #[test]
    fn terminating_overrides() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d", "deletionTimestamp": "2026-07-03T00:00:00Z"},
            "spec": {"containers": [{"name": "a"}]},
            "status": {"phase": "Running", "containerStatuses": [
                {"name": "a", "ready": true, "restartCount": 0, "state": {"running": {}}}
            ]}
        }));
        assert_eq!(pod_row(&p).status, "Terminating");
    }

    #[test]
    fn succeeded_shows_completed() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "job-1", "namespace": "d"},
            "spec": {"containers": [{"name": "a"}]},
            "status": {"phase": "Succeeded"}
        }));
        assert_eq!(pod_row(&p).status, "Completed");
    }

    #[test]
    fn evicted_reason_shown() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [{"name": "a"}]},
            "status": {"phase": "Failed", "reason": "Evicted"}
        }));
        assert_eq!(pod_row(&p).status, "Evicted");
    }

    #[test]
    fn container_creating_shown() {
        let p = pod(serde_json::json!({
            "metadata": {"name": "x", "namespace": "d"},
            "spec": {"containers": [{"name": "a"}]},
            "status": {"phase": "Pending", "containerStatuses": [
                {"name": "a", "ready": false, "restartCount": 0,
                 "state": {"waiting": {"reason": "ContainerCreating"}}}
            ]}
        }));
        assert_eq!(pod_row(&p).status, "ContainerCreating");
    }

    #[test]
    fn bare_pod_defaults() {
        let p = pod(serde_json::json!({"metadata": {"name": "x", "namespace": "d"}}));
        let r = pod_row(&p);
        assert_eq!(r.ready, "0/0");
        assert_eq!(r.status, "Unknown");
        assert_eq!(r.restarts, 0);
    }
}
