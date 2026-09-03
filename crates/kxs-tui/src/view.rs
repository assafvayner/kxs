use ratatui::crossterm::event::KeyEvent;

use kxs_cluster::discovery::ResourceKind;

use crate::cmd::Cmd;
use crate::theme::Theme;
use crate::AppCtx;

pub type ViewId = u64;

pub struct Hint {
    pub key: &'static str,
    pub desc: &'static str,
}

/// The resource row a view's keys act on (used by the app for `d`/`y`/`e`).
#[derive(Debug, Clone)]
pub struct Target {
    pub kind: ResourceKind,
    pub ns: Option<String>,
    pub name: String,
    /// Selected container, when the view is scoped to one.
    pub container: Option<String>,
}

pub trait View {
    fn id(&self) -> ViewId;
    /// e.g. "Pods(default)[42]"
    fn title(&self) -> String;
    /// e.g. "pods"
    fn crumb(&self) -> String;
    /// Header hotkey column entries; shown only for the top view.
    fn hints(&self) -> Vec<Hint> {
        vec![]
    }
    fn handle_key(&mut self, key: KeyEvent, ctx: &AppCtx) -> Vec<Cmd>;
    /// The view's background task started; it owns the stop handle now.
    fn on_started(&mut self, handle: crate::cmd::StopHandle, ctx: &AppCtx) -> Vec<Cmd> {
        let _ = (handle, ctx);
        vec![]
    }
    /// Late results for this view. Only called while the view is on the stack.
    fn on_msg(&mut self, msg: &crate::msg::Msg, ctx: &AppCtx) -> Vec<Cmd> {
        let _ = (msg, ctx);
        vec![]
    }
    fn render(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect, th: &Theme, filter: &str);
    /// Stop handles for owned streams, on quit and on pop.
    fn on_pop(&mut self) -> Vec<Cmd> {
        vec![]
    }
    /// Whether the `/` filter applies to this view (Help: no).
    fn wants_filter(&self) -> bool {
        true
    }
    /// The selected resource row, if this view is a resource listing.
    fn target(&self) -> Option<Target> {
        None
    }
    /// Views like Logs consume `0`-`5` for their own presets, overriding the
    /// global namespace favorites while they are the top view.
    fn handles_digits(&self) -> bool {
        false
    }
    /// Current filter text, shown in the title row.
    fn filter(&self) -> String {
        String::new()
    }
    fn set_filter(&mut self, filter: &str) -> Vec<Cmd> {
        let _ = filter;
        vec![]
    }
}
