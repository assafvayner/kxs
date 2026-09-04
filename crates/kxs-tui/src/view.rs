use ratatui::crossterm::event::KeyEvent;

use kxs_cluster::discovery::ResourceKind;

use crate::cmd::Cmd;
use crate::theme::Theme;
use crate::AppCtx;

pub type ViewId = u64;

pub struct Hint {
    pub key: &'static str,
    pub desc: &'static str,
    /// Hidden when `--readonly` is active.
    pub mutating: bool,
}

impl Hint {
    pub fn action(key: &'static str, desc: &'static str) -> Self {
        Hint {
            key,
            desc,
            mutating: false,
        }
    }

    pub fn mutation(key: &'static str, desc: &'static str) -> Self {
        Hint {
            key,
            desc,
            mutating: true,
        }
    }
}

/// The resource row a view's keys act on (used by the app for `d`/`y`/`e`).
#[derive(Debug, Clone)]
pub struct Target {
    pub kind: ResourceKind,
    pub ns: Option<String>,
    pub name: String,
    /// Selected container, when the view is scoped to one.
    pub container: Option<String>,
    /// Current desired replicas (Resources READY cell "2/2" → 2), for the
    /// scale input prefill.
    pub desired_replicas: Option<i32>,
    /// SUSPEND column value for CronJob rows.
    pub suspend: Option<bool>,
    /// `spec.unschedulable` for Node rows, so `u` can toggle cordon.
    pub unschedulable: Option<bool>,
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
    /// Whether the app should intercept Enter for this view's target
    /// (drill-down). Views with their own Enter semantics opt out.
    fn wants_enter(&self) -> bool {
        false
    }
    /// Views like Logs consume `0`-`5` for their own presets, overriding the
    /// global namespace favorites while they are the top view.
    fn handles_digits(&self) -> bool {
        false
    }
    /// `ctrl-w`: keep every column even when the terminal is narrow.
    /// Returns whether the view has wide columns to toggle.
    fn toggle_wide(&mut self) -> Option<bool> {
        None
    }
    /// `ctrl-z`: show only rows in a faulty state. Returns the new state, or
    /// `None` when the view has no notion of faults.
    fn toggle_faults(&mut self) -> Option<bool> {
        None
    }
    /// `f`: fill the frame, hiding the header. Returns the new state.
    fn toggle_fullscreen(&mut self) -> Option<bool> {
        None
    }
    /// Current filter text, shown in the title row.
    fn filter(&self) -> String {
        String::new()
    }
    fn set_filter(&mut self, filter: &str) -> Vec<Cmd> {
        let _ = filter;
        vec![]
    }
    /// Views that handle Esc themselves (ThemePicker reverts its preview).
    fn wants_esc(&self) -> bool {
        false
    }
}

/// Title suffix for a watch status event. `live`/`connected` clear the
/// indicator, `error` shows the message, and everything else (e.g.
/// `reconnecting`) shows a reconnecting indicator with the message.
pub fn status_suffix(state: &str, message: Option<&str>) -> Option<String> {
    match (state, message) {
        ("live", _) | ("connected", _) => None,
        ("error", Some(m)) => Some(format!("⟳ {}", short(m))),
        ("error", None) => Some("⟳ error".into()),
        (_, Some(m)) => Some(format!("⟳ reconnecting: {}", short(m))),
        (_, None) => Some("⟳ reconnecting".into()),
    }
}

/// Sanitize a status message for embedding in a title: first line only,
/// capped at 60 chars with a trailing `…` when truncated.
fn short(m: &str) -> String {
    let first_line = m.split('\n').next().unwrap_or(m);
    let has_more_lines = first_line.len() != m.len();
    let truncated_len = first_line.chars().count() > 60;
    let mut out: String = first_line.chars().take(60).collect();
    if truncated_len || has_more_lines {
        out.push('…');
    }
    out
}
