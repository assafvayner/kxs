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
    Fetched {
        view: ViewId,
        result: Result<FetchResult, String>,
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
