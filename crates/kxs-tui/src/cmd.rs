use kxs_cluster::discovery::ResourceKind;
use tokio::sync::oneshot;

use crate::view::ViewId;

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
}

/// Result payloads for `Cmd::Fetch`.
#[derive(Debug, Clone)]
pub enum FetchResult {
    Yaml(String),
    Describe(String),
    Namespaces(Vec<String>),
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
    Fetch {
        view: ViewId,
        what: Fetch,
    },
    /// Background reachability ping for one context.
    Ping {
        context: String,
    },
    /// Namespace switch requested by a view; the runtime applies it via App
    /// and records the favorite.
    SwitchNamespace {
        ns: Option<String>,
    },
    Stop(StopHandle),
    SaveConfig,
    Quit,
}
