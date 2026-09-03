use kxs_cluster::discovery::ResourceKind;
use kxs_cluster::logs::LogRequest;
use tokio::sync::oneshot;

use crate::view::ViewId;

use std::time::Duration;

/// Handle for cancelling a view's background stream. `stop` is idempotent;
/// a send to an already-dropped receiver is a no-op.
#[derive(Debug)]
pub struct StopHandle(pub oneshot::Sender<()>);

impl StopHandle {
    pub fn stop(self) {
        let _ = self.0.send(());
    }
}

#[derive(Debug, Clone)]
pub enum Fetch {
    Yaml {
        kind: ResourceKind,
        ns: Option<String>,
        name: String,
    },
    Describe {
        kind: ResourceKind,
        ns: Option<String>,
        name: String,
    },
    Namespaces,
    Containers {
        ns: String,
        pod: String,
    },
    /// Label selector of a pod owner (Job pods for a CronJob).
    WorkloadSelector {
        kind: ResourceKind,
        ns: String,
        name: String,
    },
    /// Pod names selected by a pod owner, for multi-pod logs.
    WorkloadPods {
        kind: ResourceKind,
        ns: String,
        name: String,
    },
}

/// Result payloads for `Cmd::Fetch`.
#[derive(Debug, Clone)]
pub enum FetchResult {
    Yaml(String),
    Describe(String),
    Namespaces(Vec<String>),
    Containers(Vec<kxs_cluster::pods::ContainerInfo>),
    Selector(String),
    PodNames(Vec<String>),
    /// Result of the container picker modal.
    ContainerPicked(String),
}

/// Side effects. The runtime turns each variant into a tokio task that calls
/// the matching `kxs-cluster` function and reports back via `Msg`.
pub enum Cmd {
    Connect {
        context: String,
    },
    StartTableWatch {
        view: ViewId,
        kind: ResourceKind,
        ns: Option<String>,
        selector: Option<String>,
    },
    StartPodWatch {
        view: ViewId,
        ns: Option<String>,
        selector: Option<String>,
    },
    StartLogs {
        view: ViewId,
        req: LogRequest,
    },
    /// Repeating metrics poll for the header and the Metrics view.
    PollMetrics {
        view: ViewId,
        every: Duration,
    },
    Fetch {
        view: ViewId,
        what: Fetch,
    },
    /// Background reachability ping for one context.
    Ping {
        context: String,
    },
    /// Open the container picker modal for a pod; the choice is routed back
    /// to `view` as `Msg::Picked`.
    PickContainer {
        view: ViewId,
        ns: String,
        pod: String,
        options: Vec<(String, String)>,
    },
    /// Pop the top view (e.g. a cancelled picker).
    PopView,
    /// Namespace switch requested by a view; the runtime applies it via App
    /// and records the favorite.
    SwitchNamespace {
        ns: Option<String>,
    },
    Stop(StopHandle),
    SaveConfig,
    Quit,
}
