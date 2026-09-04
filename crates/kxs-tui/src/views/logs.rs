//! Logs view: one stream per target pod, ring buffer, window presets,
//! container picker via the Chrome modal when a pod has several.

use std::cell::Cell;
use std::collections::VecDeque;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use kxs_cluster::logopts::{default_tail, log_window, SINCE_OPTIONS};
use kxs_cluster::logs::LogRequest;

use crate::clipboard;
use crate::cmd::{Cmd, Fetch, StopHandle};
use crate::theme::Theme;
use crate::view::{Hint, Target, View};
use crate::AppCtx;

const RING_CAPACITY: usize = 50_000;

/// One streamed log target.
#[derive(Debug, Clone)]
pub struct LogTarget {
    pub ns: String,
    pub pod: String,
    pub container: Option<String>,
}

pub struct LogsView {
    id: u64,
    targets: Vec<LogTarget>,
    /// Namespace of the pod or workload this view was opened for; used to
    /// stream pods resolved later (e.g. via `PodNames`) in the right namespace.
    ns: String,
    /// Workload whose pod names are still being resolved (`l` on an owner).
    pending_workload: Option<Target>,
    /// Waiting for the container picker to resolve.
    pending_pick: bool,
    /// Log all containers (shift-l).
    all_containers: bool,
    lines: VecDeque<String>,
    /// Pod name per line (multi-pod mode), parallel to `lines`.
    prefixes: VecDeque<Option<String>>,
    autoscroll: bool,
    /// Lines scrolled back from the tail; `0` means pinned to the end.
    scroll_from_end: usize,
    viewport: Cell<usize>,
    fullscreen: bool,
    wrap: bool,
    timestamps: bool,
    previous: bool,
    since_idx: usize,
    filter: String,
    handles: Vec<StopHandle>,
    status: Option<String>,
    /// A fetch failed; stop retrying until the streams are restarted.
    fetch_failed: bool,
}

impl LogsView {
    /// Single pod (`l` on a Pod row). Container resolution happens on stream
    /// start: single container → use it; several → picker; all → shift-l.
    pub fn new(app: &mut crate::app::App, target: Target, all_containers: bool) -> Self {
        let ns = target.ns.clone().unwrap_or_default();
        let target = LogTarget {
            ns: ns.clone(),
            pod: target.name.clone(),
            container: None,
        };
        LogsView {
            id: app.alloc_id(),
            targets: vec![target],
            ns,
            pending_workload: None,
            pending_pick: false,
            all_containers,
            lines: VecDeque::new(),
            prefixes: VecDeque::new(),
            autoscroll: true,
            scroll_from_end: 0,
            viewport: Cell::new(20),
            fullscreen: false,
            wrap: true,
            timestamps: false,
            previous: false,
            since_idx: SINCE_OPTIONS.len() - 1, // "all"
            filter: String::new(),
            handles: vec![],
            status: None,
            fetch_failed: false,
        }
    }

    /// Logs for a known container (`l` on a Containers row).
    /// Opens straight onto the previous container's logs (`p` from a table).
    pub fn previous(mut self) -> Self {
        self.previous = true;
        self
    }

    pub fn new_with_container(app: &mut crate::app::App, target: Target) -> Self {
        let mut v = Self::new(app, target.clone(), false);
        v.targets[0].container = target.container.clone();
        v
    }

    /// Multi-pod (`l` on a pod owner): resolve pod names first.
    pub fn new_workload(app: &mut crate::app::App, target: Target) -> Self {
        let mut v = Self::new(app, target.clone(), false);
        v.targets = vec![];
        v.pending_workload = Some(target);
        v
    }

    fn multi(&self) -> bool {
        self.targets.len() > 1
    }

    fn stop_streams(&mut self) -> Vec<Cmd> {
        std::mem::take(&mut self.handles)
            .into_iter()
            .map(Cmd::Stop)
            .collect()
    }

    fn start_streams(&mut self) -> Vec<Cmd> {
        let (tail, since) = log_window(
            SINCE_OPTIONS[self.since_idx].seconds,
            default_tail(self.multi()),
        );
        self.targets
            .iter()
            .map(|t| Cmd::StartLogs {
                view: self.id,
                req: LogRequest {
                    namespace: t.ns.clone(),
                    pod: t.pod.clone(),
                    container: t.container.clone(),
                    follow: true,
                    tail_lines: tail,
                    since_seconds: since,
                    timestamps: self.timestamps,
                    previous: self.previous,
                },
            })
            .collect()
    }

    fn restart(&mut self) -> Vec<Cmd> {
        let mut cmds = self.stop_streams();
        self.lines.clear();
        self.prefixes.clear();
        self.scroll_from_end = 0;
        self.fetch_failed = false;
        cmds.extend(self.start_streams());
        cmds
    }

    fn scroll_by(&mut self, delta: isize) {
        let inner = self.viewport.get().max(1);
        let total = self.lines.iter().filter(|l| self.line_matches(l)).count();
        let max_back = total.saturating_sub(inner);
        let next = (self.scroll_from_end as isize + delta).clamp(0, max_back as isize) as usize;
        self.scroll_from_end = next;
        self.autoscroll = next == 0 && delta <= 0 && self.autoscroll;
    }

    fn push_lines(&mut self, pod: &str, lines: &[String]) {
        let prefix = if self.multi() {
            Some(pod.to_string())
        } else {
            None
        };
        for l in lines {
            if self.lines.len() == RING_CAPACITY {
                self.lines.pop_front();
                self.prefixes.pop_front();
            }
            self.lines.push_back(l.clone());
            self.prefixes.push_back(prefix.clone());
        }
    }

    fn pod_prefix_color(pod: &str, th: &Theme) -> ratatui::style::Color {
        // stable per-pod color from a small palette
        let palette = [
            th.colors.accent,
            th.colors.green,
            th.colors.yellow,
            th.colors.syn_string,
            th.colors.syn_number,
            th.colors.red,
        ];
        let hash: usize = pod.bytes().map(|b| b as usize).sum();
        palette[hash % palette.len()]
    }

    fn line_matches(&self, line: &str) -> bool {
        self.filter.is_empty() || line.to_lowercase().contains(&self.filter.to_lowercase())
    }
}

impl View for LogsView {
    fn id(&self) -> u64 {
        self.id
    }

    fn toggle_fullscreen(&mut self) -> Option<bool> {
        self.fullscreen = !self.fullscreen;
        Some(self.fullscreen)
    }

    fn title(&self) -> String {
        let what = if self.multi() {
            format!("{} pods", self.targets.len())
        } else {
            self.targets
                .first()
                .map(|t| t.pod.clone())
                .unwrap_or_default()
        };
        let mut title = format!("Logs({what})[{}]", self.lines.len());
        title.push_str(&format!(
            "  since={} wrap={} ts={}",
            SINCE_OPTIONS[self.since_idx].label,
            if self.wrap { "on" } else { "off" },
            if self.timestamps { "on" } else { "off" }
        ));
        if self.previous {
            title.push_str(" prev");
        }
        if let Some(s) = &self.status {
            title.push_str(&format!("  {s}"));
        }
        title
    }

    fn crumb(&self) -> String {
        "logs".into()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![
            Hint::action("s", "autoscroll"),
            Hint::action("f", "fullscreen"),
            Hint::action("w", "wrap"),
            Hint::action("t", "timestamps"),
            Hint::action("c", "clear"),
            Hint::action("p", "previous"),
            Hint::action("0-5", "since"),
            Hint::action("shift-c", "copy"),
        ]
    }

    /// The logs view consumes `0`–`5` for since presets, overriding the
    /// global namespace favorites while it is the top view.
    fn handles_digits(&self) -> bool {
        true
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        match key.code {
            KeyCode::Char('s') => {
                self.autoscroll = !self.autoscroll;
                if self.autoscroll {
                    self.scroll_from_end = 0;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_by(1);
                self.autoscroll = false;
            }
            KeyCode::Down | KeyCode::Char('j') => self.scroll_by(-1),
            KeyCode::PageUp => {
                self.scroll_by(self.viewport.get() as isize);
                self.autoscroll = false;
            }
            KeyCode::PageDown => self.scroll_by(-(self.viewport.get() as isize)),
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_by(self.viewport.get() as isize);
                self.autoscroll = false;
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_by(-(self.viewport.get() as isize))
            }
            KeyCode::Char('g') => {
                self.scroll_by(isize::MAX / 2);
                self.autoscroll = false;
            }
            KeyCode::Char('G') => {
                self.scroll_from_end = 0;
                self.autoscroll = true;
            }
            KeyCode::Char('w') => self.wrap = !self.wrap,
            KeyCode::Char('t') => {
                self.timestamps = !self.timestamps;
                return self.restart();
            }
            KeyCode::Char('c') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.lines.clear();
                self.prefixes.clear();
            }
            KeyCode::Char('C') => {
                let visible: Vec<&str> = self
                    .lines
                    .iter()
                    .filter(|l| self.line_matches(l))
                    .map(String::as_str)
                    .collect();
                clipboard::copy(&visible.join("\n"));
            }
            KeyCode::Char('p') => {
                self.previous = !self.previous;
                return self.restart();
            }
            KeyCode::Char(c @ '0'..='5') => {
                self.since_idx = c as usize - '0' as usize;
                return self.restart();
            }
            _ => {}
        }
        vec![]
    }

    fn on_msg(&mut self, msg: &crate::msg::Msg, _ctx: &AppCtx) -> Vec<Cmd> {
        match msg {
            crate::msg::Msg::Tick => {
                if self.fetch_failed {
                    return vec![];
                }
                if !self.handles.is_empty() || self.pending_pick {
                    return vec![];
                }
                if let Some(t) = self.pending_workload.take() {
                    // resolve the workload's pods, then stream one per pod
                    return vec![Cmd::Fetch {
                        view: self.id,
                        what: Fetch::WorkloadPods {
                            kind: t.kind.clone(),
                            ns: t.ns.clone().unwrap_or_default(),
                            name: t.name.clone(),
                        },
                    }];
                }
                if self.targets.is_empty() {
                    return vec![];
                }
                if self.multi() {
                    return self.start_streams();
                }
                // single pod: resolve containers, pick when several
                self.pending_pick = true;
                vec![Cmd::Fetch {
                    view: self.id,
                    what: Fetch::Containers {
                        ns: self.targets[0].ns.clone(),
                        pod: self.targets[0].pod.clone(),
                    },
                }]
            }
            crate::msg::Msg::Fetched { result, .. } => match result {
                Ok(crate::cmd::FetchResult::Containers(infos)) => {
                    self.pending_pick = false;
                    let exec: Vec<kxs_cluster::pods::ContainerInfo> = infos
                        .iter()
                        .filter(|c| !c.init_container)
                        .cloned()
                        .collect();
                    if self.all_containers {
                        // one stream per container
                        self.targets = exec
                            .iter()
                            .map(|c| LogTarget {
                                ns: self.ns.clone(),
                                pod: self.targets[0].pod.clone(),
                                container: Some(c.name.clone()),
                            })
                            .collect();
                        return self.start_streams();
                    }
                    if exec.len() <= 1 {
                        self.targets[0].container = exec.first().map(|c| c.name.clone());
                        return self.start_streams();
                    }
                    // several containers: open the picker
                    self.pending_pick = true;
                    let options = exec
                        .iter()
                        .map(|c| {
                            let hint = if c.ready {
                                c.image.clone()
                            } else {
                                format!("{} · not ready", c.image)
                            };
                            (c.name.clone(), hint)
                        })
                        .collect();
                    vec![Cmd::PickContainer {
                        view: self.id,
                        ns: self.targets[0].ns.clone(),
                        pod: self.targets[0].pod.clone(),
                        options,
                    }]
                }
                Ok(crate::cmd::FetchResult::ContainerPicked(name)) => {
                    self.pending_pick = false;
                    self.targets[0].container = Some(name.clone());
                    self.start_streams()
                }
                Ok(crate::cmd::FetchResult::PodNames(names)) => {
                    self.targets = names
                        .iter()
                        .map(|pod| LogTarget {
                            ns: self.ns.clone(),
                            pod: pod.clone(),
                            container: None,
                        })
                        .collect();
                    self.start_streams()
                }
                Err(e) => {
                    self.pending_pick = false;
                    self.fetch_failed = true;
                    self.status = Some(e.clone());
                    vec![]
                }
                _ => vec![],
            },
            crate::msg::Msg::Picked { choice, .. } => {
                self.pending_pick = false;
                match choice {
                    Some(name) => {
                        self.targets[0].container = Some(name.clone());
                        self.start_streams()
                    }
                    None => vec![Cmd::PopView],
                }
            }
            crate::msg::Msg::LogLines { pod, lines, .. } => {
                self.push_lines(pod, lines);
                self.status = None;
                vec![]
            }
            crate::msg::Msg::LogStatus { pod, status, .. } => {
                self.status = Some(match status {
                    Ok(()) => format!("{pod}: eof"),
                    Err(e) => format!("✗ {pod}: {e}"),
                });
                vec![]
            }
            _ => vec![],
        }
    }

    fn set_filter(&mut self, filter: &str) -> Vec<Cmd> {
        self.filter = filter.to_string();
        vec![]
    }

    fn filter(&self) -> String {
        self.filter.clone()
    }

    fn on_started(&mut self, handle: StopHandle, _ctx: &AppCtx) -> Vec<Cmd> {
        self.handles.push(handle);
        vec![]
    }

    fn on_pop(&mut self) -> Vec<Cmd> {
        self.stop_streams()
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, _filter: &str) {
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(th.colors.border))
            .title(Line::from(Span::styled(
                self.title(),
                Style::new().fg(th.colors.accent),
            )));
        let inner_h = area.height.saturating_sub(2) as usize;
        self.viewport.set(inner_h);
        let all: Vec<(&Option<String>, &String)> = self
            .prefixes
            .iter()
            .zip(self.lines.iter())
            .filter(|(_, l)| self.line_matches(l))
            .collect();
        if self.lines.is_empty() {
            f.render_widget(block, area);
            f.render_widget(
                Paragraph::new("waiting for logs…").style(Style::new().fg(th.colors.fg_dim)),
                Rect {
                    x: area.x + 2,
                    y: area.y + 1,
                    width: area.width.saturating_sub(4),
                    height: 1,
                },
            );
            return;
        }
        let total = all.len();
        let start = total.saturating_sub(inner_h + self.scroll_from_end);
        let end = total.saturating_sub(self.scroll_from_end).max(start);
        let visible = &all[start..end];
        let mut lines: Vec<Line> = Vec::with_capacity(visible.len());
        for (prefix, text) in visible {
            let mut spans: Vec<Span> = Vec::new();
            if let Some(p) = prefix {
                spans.push(Span::styled(
                    format!("{p} "),
                    Style::new().fg(Self::pod_prefix_color(p, th)),
                ));
            }
            spans.push(Span::styled((*text).clone(), Style::new().fg(th.colors.fg)));
            lines.push(Line::from(spans));
        }
        let mut p = Paragraph::new(lines).block(block);
        if self.wrap {
            p = p.wrap(Wrap { trim: false });
        }
        f.render_widget(p, area);
    }
}
