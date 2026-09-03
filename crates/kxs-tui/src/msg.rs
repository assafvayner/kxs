use kxs_cluster::metrics::{MetricsRow, NodeMetricsRow};
use kxs_cluster::pods::PodEvent;
use kxs_cluster::resources::TableEvent;
use ratatui::crossterm::event::KeyEvent;

use crate::cmd::FetchResult;
use crate::cmd::StopHandle;
use crate::view::ViewId;

/// Every state change enters through `App::update` as a `Msg`.
pub enum Msg {
    Key(KeyEvent),
    Resize(u16, u16),
    Tick,
    /// A view's background task started; carries its stop handle.
    Started {
        view: ViewId,
        handle: StopHandle,
    },
    Table {
        view: ViewId,
        ev: TableEvent,
    },
    Pod {
        view: ViewId,
        ev: PodEvent,
    },
    /// Batched log lines from one pod's stream (the stream closure knows which).
    LogLines {
        view: ViewId,
        pod: String,
        lines: Vec<String>,
    },
    /// Stream terminal state for one pod: EOF or an error message.
    LogStatus {
        view: ViewId,
        pod: String,
        status: Result<(), String>,
    },
    /// Periodic metrics-server poll; feeds the header CPU/MEM line too.
    Metrics {
        view: ViewId,
        pods: Result<Vec<MetricsRow>, String>,
        nodes: Result<Vec<NodeMetricsRow>, String>,
    },
    Fetched {
        view: ViewId,
        result: Result<FetchResult, String>,
    },
    /// Resolution of a Chrome pick modal; `None` = cancelled.
    Picked {
        view: ViewId,
        choice: Option<String>,
    },
    /// Background reachability ping for one context (Contexts view rows).
    Pinged {
        context: String,
        result: Result<String, String>,
    },
    Connected {
        context: String,
        result: Result<String, String>,
    },
    Error {
        view: Option<ViewId>,
        text: String,
    },
    KubeconfigChanged,
}
