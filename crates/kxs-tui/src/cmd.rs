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
    RolloutHistory {
        ns: String,
        name: String,
    },
}

/// A cluster mutation; the runtime dispatches to the matching kxs-cluster call.
#[derive(Debug, Clone)]
pub enum Mutation {
    Scale {
        kind: ResourceKind,
        ns: String,
        name: String,
        replicas: i32,
    },
    Restart {
        kind: ResourceKind,
        ns: String,
        name: String,
    },
    Cordon {
        ns: String,
        name: String,
        unschedulable: bool,
    },
    Drain {
        ns: String,
        name: String,
    },
    Trigger {
        ns: String,
        name: String,
    },
    Suspend {
        ns: String,
        name: String,
        suspend: bool,
    },
    Undo {
        ns: String,
        name: String,
        revision: i64,
    },
    Delete {
        kind: ResourceKind,
        ns: String,
        name: String,
        propagation: Option<String>,
        force: bool,
    },
}

/// Terminal handoff actions, run inline by the runtime with the TUI suspended.
#[derive(Debug, Clone)]
pub enum SuspendAction {
    /// `e`: fetch the manifest, open $KUBE_EDITOR, apply on save.
    Edit {
        kind: ResourceKind,
        ns: Option<String>,
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
    Rollout(Vec<kxs_cluster::workloads::RolloutRevision>),
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
    /// A cluster mutation (delete/scale/restart/...); reports via `Msg::Mutated`.
    Mutate {
        view: ViewId,
        m: Mutation,
    },
    /// Leave the TUI, run the action on the real terminal, come back.
    Suspend(SuspendAction),
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
    /// Ask (via a confirm modal) to roll back to a revision.
    ConfirmUndo {
        view: ViewId,
        ns: String,
        name: String,
        revision: i64,
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
