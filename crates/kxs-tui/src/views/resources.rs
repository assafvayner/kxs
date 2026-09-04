//! Generic resource table driven by `run_table_watch`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use ratatui::Frame;
use std::cell::Cell;

use kxs_cluster::discovery::ResourceKind;
use kxs_cluster::resources::{ResourceRow, ResourceTable, TableEvent};
use kxs_cluster::table::{
    cycle_sort, filter_predicate, sort_indicator, sort_rows, split_filter, Sort,
};

use crate::cmd::{Cmd, StopHandle};
use crate::select::move_selection;
use crate::theme::Theme;
use crate::view::{Hint, View};
use crate::AppCtx;

pub struct ResourcesView {
    id: u64,
    kind: ResourceKind,
    /// Namespace the watch was started with; `None` = all namespaces.
    watched_ns: Option<String>,
    filter: String,
    labels: Option<String>,
    name_filter: String,
    /// `ctrl-z`: show only rows whose status reads as a fault.
    faults_only: bool,
    /// `ctrl-w`: keep every column even when the terminal is narrow.
    wide: bool,
    table: Option<ResourceTable>,
    /// Sort key is the original cell index; `cells.len()` is the synthetic AGE column.
    sort: Option<Sort>,
    selected: Option<String>,
    handle: Option<StopHandle>,
    /// Watch requested but `Started` not yet received.
    pending: bool,
    /// Awaiting container resolution for exec / port-forward.
    pending_exec: bool,
    /// Awaiting container resolution for `a` attach.
    pending_attach: bool,
    pending_pf: bool,
    pf_ns_pod: (String, String),
    status: Option<String>,
    viewport_rows: Cell<u16>,
    scroll: crate::table::Scroll,
}

impl ResourcesView {
    pub fn new(app: &mut crate::app::App, kind: ResourceKind, ns: Option<String>) -> Self {
        ResourcesView {
            id: app.alloc_id(),
            kind,
            watched_ns: ns,
            filter: String::new(),
            labels: None,
            name_filter: String::new(),
            faults_only: false,
            wide: false,
            table: None,
            sort: None,
            selected: None,
            handle: None,
            pending: false,
            pending_exec: false,
            pending_attach: false,
            pending_pf: false,
            pf_ns_pod: (String::new(), String::new()),
            status: None,
            viewport_rows: Cell::new(20),
            scroll: Default::default(),
        }
    }

    /// Display order of original cell indexes, AGE forced last.
    fn column_map(&self) -> Vec<usize> {
        let Some(t) = &self.table else { return vec![] };
        let mut idx: Vec<usize> = (0..t.columns.len()).collect();
        if let Some(i) = t.columns.iter().position(|c| is_age_col(c)) {
            if i != idx.len() - 1 {
                idx.remove(i);
                idx.push(i);
            }
        }
        idx
    }

    fn sort_key_for_display(&self, display: usize) -> usize {
        let Some(t) = &self.table else { return display };
        let map = self.column_map();
        match map.get(display) {
            Some(&ci) if is_age_col(&t.columns[ci]) => t.columns.len(),
            Some(&ci) => ci,
            None => display,
        }
    }

    /// Cell text for display column `ci`; the synthetic Age column is rendered
    /// from `created`.
    fn cell_text<'a>(
        &self,
        r: &'a ResourceRow,
        ci: usize,
        now_ms: i64,
    ) -> std::borrow::Cow<'a, str> {
        let is_age = self
            .table
            .as_ref()
            .and_then(|t| t.columns.get(ci))
            .is_some_and(|c| is_age_col(c));
        if is_age {
            std::borrow::Cow::Owned(kxs_core::format::age(r.created.as_deref(), now_ms))
        } else {
            std::borrow::Cow::Borrowed(r.cells.get(ci).map(String::as_str).unwrap_or(""))
        }
    }

    /// Display columns as (original cell index, width). Widths are
    /// max(header, widest cell) capped at 60. When the sum exceeds the area,
    /// rightmost non-NAME non-AGE columns are dropped; NAME absorbs the
    /// remaining slack. AGE is always kept last.
    fn layout(&self, total: u16, now_ms: i64) -> Vec<(usize, u16)> {
        let Some(t) = &self.table else { return vec![] };
        let map = self.column_map();
        let mut cols: Vec<(usize, u16)> = map
            .iter()
            .map(|&ci| {
                let hdr = t.columns[ci].chars().count() as u16;
                let widest = t
                    .rows
                    .iter()
                    .map(|r| self.cell_text(r, ci, now_ms).chars().count() as u16)
                    .max()
                    .unwrap_or(0);
                (ci, hdr.max(widest).min(60))
            })
            .collect();
        if cols.is_empty() {
            return cols;
        }
        // borders + one space between columns
        let overhead = |n: usize| 2u16 + (n as u16).saturating_sub(1);
        while !self.wide && cols.len() > 2 {
            let n = cols.len();
            let needed: u16 = cols.iter().map(|(_, w)| *w).sum::<u16>() + overhead(n);
            if needed <= total {
                break;
            }
            cols.remove(n - 2);
        }
        if cols.len() > 2 {
            let n = cols.len();
            let others: u16 = cols[1..].iter().map(|(_, w)| *w).sum();
            let avail = total.saturating_sub(overhead(n));
            cols[0].1 = avail.saturating_sub(others).max(4);
        }
        cols
    }

    /// Rows after filtering, sorted per the active sort.
    fn visible_rows(&self) -> Vec<ResourceRow> {
        let Some(t) = &self.table else { return vec![] };
        let pred = filter_predicate(&self.name_filter);
        let rows: Vec<ResourceRow> = t
            .rows
            .iter()
            .filter(|r| pred(&r.name))
            .filter(|r| !self.faults_only || row_is_faulty(r))
            .cloned()
            .collect();
        match self.sort {
            Some(s) => sort_rows(&rows, s.key, s.dir),
            None => rows,
        }
    }

    fn keys(&self) -> Vec<String> {
        self.visible_rows().iter().map(|r| r.key.clone()).collect()
    }

    fn selected_index(&self) -> Option<usize> {
        let sel = self.selected.as_deref()?;
        self.visible_rows().iter().position(|r| r.key == sel)
    }

    /// Keep the selection on a visible row: unchanged if still visible, else the
    /// first visible row, else none.
    fn fix_selection(&mut self) {
        let keys = self.keys();
        if !keys
            .iter()
            .any(|k| Some(k.as_str()) == self.selected.as_deref())
        {
            self.selected = keys.first().cloned();
        }
    }

    fn target_of(&self, r: &ResourceRow) -> crate::view::Target {
        crate::view::Target {
            kind: self.kind.clone(),
            // the row's own namespace, not the watched one: in an "all
            // namespaces" view the selected row still lives in a specific ns
            ns: r.namespace.clone().or_else(|| self.watched_ns.clone()),
            name: r.name.clone(),
            container: None,
            // READY "2/2" → desired replicas 2 (deployment-style rows)
            desired_replicas: r
                .cells
                .iter()
                .find(|c| c.contains('/') && c.split('/').all(|p| p.trim().parse::<i32>().is_ok()))
                .and_then(|c| c.split('/').next()?.trim().parse::<i32>().ok()),
            suspend: r
                .cells
                .iter()
                .find(|c| c.eq_ignore_ascii_case("true") || c.eq_ignore_ascii_case("false"))
                .map(|c| c.eq_ignore_ascii_case("true")),
            // Node STATUS reads "Ready,SchedulingDisabled" when cordoned
            unschedulable: (self.kind.kind == "Node")
                .then(|| r.cells.iter().any(|c| c.contains("SchedulingDisabled"))),
        }
    }

    fn restart_watch(&mut self) -> Cmd {
        self.pending = true;
        Cmd::StartTableWatch {
            view: self.id,
            kind: self.kind.clone(),
            ns: self.watched_ns.clone(),
            selector: self.labels.clone(),
        }
    }

    fn stop_old(&mut self) -> Vec<Cmd> {
        match self.handle.take() {
            Some(h) => vec![Cmd::Stop(h)],
            None => vec![],
        }
    }
}

/// Resolves a kind query against the active session's discovery and builds the
/// view. `None` for an unknown kind.
pub fn open(app: &mut crate::app::App, query: &str, ns: Option<String>) -> Option<Box<dyn View>> {
    let kinds = app.ctx().kinds;
    let kind = kxs_cluster::command::resolve_kind(&kinds, query)?;
    let ns = ns.or_else(|| app.ctx().namespace);
    Some(Box::new(ResourcesView::new(app, kind, ns)))
}

impl View for ResourcesView {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        let count = self.visible_rows().len();
        let ns = match &self.watched_ns {
            Some(n) => n.as_str(),
            None => "all",
        };
        let mut title = format!("{}({})[{}]", self.kind.kind, ns, count);
        if let Some(status) = &self.status {
            title.push_str(&format!("  {status}"));
        }
        title
    }

    fn crumb(&self) -> String {
        self.kind.plural.clone()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![
            Hint::action("d", "describe"),
            Hint::action("y", "yaml"),
            Hint::action("ctrl-r", "refresh"),
        ]
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        let keys = self.keys();
        let page = self.viewport_rows.get().max(1) as isize;
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = move_selection(&keys, self.selected.as_deref(), 1);
                vec![]
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = move_selection(&keys, self.selected.as_deref(), -1);
                vec![]
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected = keys.first().cloned();
                vec![]
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected = keys.last().cloned();
                vec![]
            }
            KeyCode::PageDown => {
                self.selected = move_selection(&keys, self.selected.as_deref(), page);
                vec![]
            }
            KeyCode::PageUp => {
                self.selected = move_selection(&keys, self.selected.as_deref(), -page);
                vec![]
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.selected = move_selection(&keys, self.selected.as_deref(), page);
                vec![]
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.selected = move_selection(&keys, self.selected.as_deref(), -page);
                vec![]
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let mut cmds = self.stop_old();
                cmds.push(self.restart_watch());
                cmds
            }
            KeyCode::Char(c) if c.is_ascii_uppercase() => {
                let Some(t) = &self.table else { return vec![] };
                let map = self.column_map();
                let Some(display) = map
                    .iter()
                    .position(|&ci| t.columns[ci].trim().to_uppercase().starts_with(c))
                else {
                    return vec![];
                };
                let key = self.sort_key_for_display(display);
                self.sort = cycle_sort(self.sort, key);
                vec![]
            }
            _ => vec![],
        }
    }

    fn on_started(&mut self, handle: StopHandle, _ctx: &AppCtx) -> Vec<Cmd> {
        self.handle = Some(handle);
        self.pending = false;
        vec![]
    }

    fn on_msg(&mut self, msg: &crate::msg::Msg, ctx: &AppCtx) -> Vec<Cmd> {
        match msg {
            crate::msg::Msg::Fetched {
                result: Ok(crate::cmd::FetchResult::AttachContainers { ns, pod, infos }),
                ..
            } => {
                self.pf_ns_pod = (ns.clone(), pod.clone());
                self.pending_attach = false;
                let attachable: Vec<kxs_cluster::pods::ContainerInfo> = infos
                    .iter()
                    .filter(|c| !c.init_container)
                    .cloned()
                    .collect();
                match attachable.len() {
                    0 => {
                        self.status = Some("no attachable containers".into());
                        vec![]
                    }
                    1 => vec![Cmd::Suspend(crate::cmd::SuspendAction::Attach {
                        ns: self.pf_ns_pod.0.clone(),
                        pod: self.pf_ns_pod.1.clone(),
                        container: Some(attachable[0].name.clone()),
                    })],
                    _ => {
                        let options = attachable
                            .iter()
                            .map(|c| (c.name.clone(), c.image.clone()))
                            .collect();
                        self.pending_attach = true;
                        vec![Cmd::PickExec {
                            view: self.id,
                            ns: self.pf_ns_pod.0.clone(),
                            pod: self.pf_ns_pod.1.clone(),
                            options,
                        }]
                    }
                }
            }
            crate::msg::Msg::Fetched {
                result: Ok(crate::cmd::FetchResult::ExecContainers { ns, pod, infos }),
                ..
            } => {
                self.pf_ns_pod = (ns.clone(), pod.clone());
                self.pending_exec = false;
                let exec: Vec<kxs_cluster::pods::ContainerInfo> = infos
                    .iter()
                    .filter(|c| !c.init_container)
                    .cloned()
                    .collect();
                match exec.len() {
                    0 => {
                        self.status = Some("no exec-able containers".into());
                        vec![]
                    }
                    1 => {
                        vec![Cmd::Suspend(crate::cmd::SuspendAction::Exec {
                            ns: self.pf_ns_pod.0.clone(),
                            pod: self.pf_ns_pod.1.clone(),
                            container: Some(exec[0].name.clone()),
                        })]
                    }
                    _ => {
                        let options = exec
                            .iter()
                            .map(|c| (c.name.clone(), c.image.clone()))
                            .collect();
                        self.pending_exec = true;
                        vec![Cmd::PickExec {
                            view: self.id,
                            ns: self.pf_ns_pod.0.clone(),
                            pod: self.pf_ns_pod.1.clone(),
                            options,
                        }]
                    }
                }
            }
            crate::msg::Msg::Fetched {
                result: Ok(crate::cmd::FetchResult::ForwardPorts { ns, pod, choices }),
                ..
            } => {
                self.pf_ns_pod = (ns.clone(), pod.clone());
                self.pending_pf = false;
                if choices.len() == 1 {
                    let c = &choices[0];
                    vec![Cmd::StartForward {
                        view: self.id,
                        ns: self.pf_ns_pod.0.clone(),
                        pod: self.pf_ns_pod.1.clone(),
                        port: c.port,
                    }]
                } else {
                    self.pending_pf = true;
                    vec![Cmd::PickExec {
                        view: self.id,
                        ns: self.pf_ns_pod.0.clone(),
                        pod: self.pf_ns_pod.1.clone(),
                        options: choices
                            .iter()
                            .map(|c| (c.port.to_string(), c.label.clone()))
                            .collect(),
                    }]
                }
            }
            crate::msg::Msg::Fetched {
                result: Ok(crate::cmd::FetchResult::Endpoint(pod, port)),
                ..
            } => {
                self.pending_pf = false;
                vec![Cmd::StartForward {
                    view: self.id,
                    ns: self.pf_ns_pod.0.clone(),
                    pod: pod.clone(),
                    port: *port,
                }]
            }
            crate::msg::Msg::Picked { choice, .. } => {
                let Some(choice) = choice else {
                    self.pending_exec = false;
                    self.pending_attach = false;
                    self.pending_pf = false;
                    return vec![];
                };
                if self.pending_attach {
                    self.pending_attach = false;
                    vec![Cmd::Suspend(crate::cmd::SuspendAction::Attach {
                        ns: self.pf_ns_pod.0.clone(),
                        pod: self.pf_ns_pod.1.clone(),
                        container: Some(choice.clone()),
                    })]
                } else if self.pending_exec {
                    self.pending_exec = false;
                    vec![Cmd::Suspend(crate::cmd::SuspendAction::Exec {
                        ns: self.pf_ns_pod.0.clone(),
                        pod: self.pf_ns_pod.1.clone(),
                        container: Some(choice.clone()),
                    })]
                } else if self.pending_pf {
                    self.pending_pf = false;
                    match choice.parse::<u16>() {
                        Ok(port) => vec![Cmd::StartForward {
                            view: self.id,
                            ns: self.pf_ns_pod.0.clone(),
                            pod: self.pf_ns_pod.1.clone(),
                            port,
                        }],
                        Err(_) => {
                            self.status = Some(format!("bad port: {choice}"));
                            vec![]
                        }
                    }
                } else {
                    vec![]
                }
            }
            crate::msg::Msg::Tick => {
                // initial watch start (issued from push_view's first tick)
                if self.handle.is_none() && !self.pending {
                    return vec![self.restart_watch()];
                }
                // follow namespace switches made outside this view
                if self.handle.is_some() && ctx.namespace != self.watched_ns {
                    self.watched_ns = ctx.namespace.clone();
                    let mut cmds = self.stop_old();
                    cmds.push(self.restart_watch());
                    return cmds;
                }
                vec![]
            }
            crate::msg::Msg::Table { ev, .. } => match ev {
                TableEvent::Table { table } => {
                    self.status = None;
                    self.table = Some(table.clone());
                    self.fix_selection();
                    vec![]
                }
                TableEvent::Status { state, message } => {
                    self.status = crate::view::status_suffix(state, message.as_deref());
                    vec![]
                }
            },
            _ => vec![],
        }
    }

    fn filter(&self) -> String {
        self.filter.clone()
    }

    fn set_filter(&mut self, filter: &str) -> Vec<Cmd> {
        self.filter = filter.to_string();
        let (labels, name) = split_filter(filter);
        let selector_changed = labels != self.labels;
        self.labels = labels;
        self.name_filter = name;
        self.fix_selection();
        if selector_changed {
            let mut cmds = self.stop_old();
            cmds.push(self.restart_watch());
            self.table = None;
            return cmds;
        }
        vec![]
    }

    fn on_pop(&mut self) -> Vec<Cmd> {
        self.stop_old()
    }

    fn wants_enter(&self) -> bool {
        true
    }

    fn toggle_wide(&mut self) -> Option<bool> {
        self.wide = !self.wide;
        Some(self.wide)
    }

    fn toggle_faults(&mut self) -> Option<bool> {
        self.faults_only = !self.faults_only;
        self.fix_selection();
        Some(self.faults_only)
    }

    fn target(&self) -> Option<crate::view::Target> {
        let rows = self.visible_rows();
        rows.iter()
            .find(|r| Some(r.key.as_str()) == self.selected.as_deref())
            .map(|r| self.target_of(r))
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, filter: &str) {
        self.viewport_rows.set(area.height.saturating_sub(2));
        let mut title = self.title();
        if !filter.is_empty() {
            title.push_str(&format!("  filter: {filter}"));
        }
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(th.colors.border))
            .title(Line::from(Span::styled(
                title,
                Style::new().fg(th.colors.accent),
            )));
        if self.table.is_none() {
            f.render_widget(block, area);
            let loading = Paragraph::new("loading…").style(Style::new().fg(th.colors.fg_dim));
            f.render_widget(
                loading,
                Rect {
                    x: area.x + 2,
                    y: area.y + 1,
                    width: area.width.saturating_sub(4),
                    height: 1,
                },
            );
            return;
        }
        let visible = self.visible_rows();
        let now_ms = kxs_cluster::clock::now_ms();
        let cols = self.layout(area.width, now_ms);
        let header_cells: Vec<Span> = cols
            .iter()
            .map(|(ci, _)| {
                let t = self.table.as_ref().expect("table");
                let key = if is_age_col(&t.columns[*ci]) {
                    t.columns.len()
                } else {
                    *ci
                };
                let indicator = sort_indicator(self.sort, key);
                Span::styled(
                    format!("{}{}", t.columns[*ci], indicator),
                    Style::new().fg(th.colors.fg_dim).bold(),
                )
            })
            .collect();
        let rows = visible.iter().map(|r| {
            let cells: Vec<Span> = cols
                .iter()
                .map(|(ci, _)| {
                    let text = self.cell_text(r, *ci, now_ms);
                    Span::styled(text, Style::new().fg(th.colors.fg))
                })
                .collect();
            Row::new(cells).style(if self.selected.as_deref() == Some(r.key.as_str()) {
                Style::new().bg(th.colors.bg_active)
            } else {
                Style::new()
            })
        });
        let constraints: Vec<Constraint> =
            cols.iter().map(|(_, w)| Constraint::Length(*w)).collect();
        let table = Table::new(rows, constraints)
            .header(Row::new(header_cells))
            .block(block);
        self.scroll.render(f, area, table, self.selected_index());
    }
}

/// Whether a column header names the synthetic AGE column.
fn is_age_col(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case("age")
}

/// `ctrl-z` predicate for a server-side table row: a healthy row is one whose
/// cells carry no failure word and whose `n/m` readiness cells are complete.
fn row_is_faulty(r: &ResourceRow) -> bool {
    const BAD: [&str; 9] = [
        "Error",
        "Failed",
        "CrashLoopBackOff",
        "ImagePullBackOff",
        "ErrImagePull",
        "Pending",
        "Unknown",
        "NotReady",
        "Evicted",
    ];
    for c in &r.cells {
        if BAD.iter().any(|b| c.contains(b)) {
            return true;
        }
        if let Some((a, b)) = c.split_once('/') {
            if let (Ok(a), Ok(b)) = (a.trim().parse::<i64>(), b.trim().parse::<i64>()) {
                if a < b {
                    return true;
                }
            }
        }
    }
    false
}
