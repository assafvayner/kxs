//! Pods view: live table over `run_pod_watch` with client-side metrics.

use std::cell::Cell;
use std::collections::HashMap;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use kxs_cluster::discovery::ResourceKind;
use kxs_cluster::metrics::MetricsRow;
use kxs_cluster::pods::PodRow;
use kxs_cluster::table::{sort_pods, PodField};
use kxs_cluster::utilization::{cpu_util, mem_util};

use crate::cmd::{Cmd, StopHandle};
use crate::select::move_selection;
use crate::theme::Theme;
use crate::view::{Hint, Target, View};
use crate::AppCtx;

/// Sort columns for the pods table; Cpu/Mem sort on the client-side metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortCol {
    Name,
    Ready,
    Status,
    Restarts,
    Cpu,
    Mem,
    Ip,
    Node,
    Age,
}

pub struct PodsView {
    id: u64,
    /// Namespace the watch was started with; `None` = all namespaces.
    watched_ns: Option<String>,
    /// Label selector for workload drill-down.
    selector: Option<String>,
    /// Waiting for `Fetch::WorkloadSelector` before starting the watch.
    selector_pending: bool,
    /// `/` filter text as typed; `-l` selectors are split out into
    /// `filter_selector` and handed to the watch.
    filter: String,
    /// Label selector from a `-l` filter, ANDed with `selector`.
    filter_selector: Option<String>,
    /// Name half of the filter, matched locally.
    name_filter: String,
    /// `ctrl-z`: show only pods that are not healthy.
    faults_only: bool,
    /// `ctrl-w`: keep the wide columns (IP/NODE) even when narrow.
    wide: bool,
    rows: Vec<PodRow>,
    /// key → latest metrics
    metrics: HashMap<String, MetricsRow>,
    sort: Option<(SortCol, kxs_cluster::table::SortDir)>,
    selected: Option<String>,
    handle: Option<StopHandle>,
    pending: bool,
    /// Awaiting container resolution for exec / port-forward.
    pending_exec: bool,
    /// Awaiting container resolution for `a` attach.
    pending_attach: bool,
    pending_pf: bool,
    pf_ns_pod: (String, String),
    /// Pod keys ("ns/name") with a live forward, for the PF column.
    forwarded: std::collections::HashSet<String>,
    status: Option<String>,
    /// Whether the watch has delivered its initial snapshot.
    loaded: bool,
    viewport_rows: Cell<u16>,
    scroll: crate::table::Scroll,
}

/// The Pod ResourceKind, resolved from discovery when available.
pub fn pod_kind(ctx: &AppCtx) -> ResourceKind {
    ctx.kinds
        .iter()
        .find(|k| k.kind == "Pod" && k.group.is_empty())
        .cloned()
        .unwrap_or_else(|| ResourceKind {
            group: String::new(),
            version: "v1".into(),
            kind: "Pod".into(),
            plural: "pods".into(),
            namespaced: true,
            aliases: vec!["po".into()],
        })
}

impl PodsView {
    pub fn new(app: &mut crate::app::App, ns: Option<String>) -> Self {
        PodsView {
            id: app.alloc_id(),
            watched_ns: ns,
            selector: None,
            selector_pending: false,
            filter: String::new(),
            filter_selector: None,
            name_filter: String::new(),
            faults_only: false,
            wide: false,
            rows: vec![],
            metrics: HashMap::new(),
            sort: None,
            selected: None,
            handle: None,
            pending: false,
            pending_exec: false,
            pending_attach: false,
            pending_pf: false,
            pf_ns_pod: (String::new(), String::new()),
            forwarded: Default::default(),
            status: None,
            loaded: false,
            viewport_rows: Cell::new(20),
            scroll: Default::default(),
        }
    }

    /// Drill-down from a pod owner: wait for the selector before watching.
    pub fn new_with_pending_selector(app: &mut crate::app::App, ns: Option<String>) -> Self {
        let mut v = PodsView::new(app, ns);
        v.selector_pending = true;
        v
    }

    fn keys(&self) -> Vec<String> {
        self.visible_rows().iter().map(|r| r.key.clone()).collect()
    }

    fn selected_index(&self) -> Option<usize> {
        let sel = self.selected.as_deref()?;
        self.visible_rows().iter().position(|r| r.key == sel)
    }

    fn restart_watch(&mut self) -> Cmd {
        self.pending = true;
        self.loaded = false;
        Cmd::StartPodWatch {
            view: self.id,
            ns: self.watched_ns.clone(),
            selector: join_selectors(self.selector.as_deref(), self.filter_selector.as_deref()),
        }
    }

    fn stop_old(&mut self) -> Vec<Cmd> {
        match self.handle.take() {
            Some(h) => vec![Cmd::Stop(h)],
            None => vec![],
        }
    }

    /// Cell text per display column (see `columns`).
    fn columns(&self, all_ns: bool) -> Vec<String> {
        let mut cols = Vec::new();
        if all_ns {
            cols.push("NAMESPACE".into());
        }
        cols.extend(
            [
                "NAME", "PF", "READY", "STATUS", "RESTARTS", "CPU", "MEM", "IP", "NODE", "AGE",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
        cols
    }

    fn cells(&self, all_ns: bool, p: &PodRow, now_ms: i64) -> Vec<String> {
        let m = self.metrics.get(&p.key);
        let cpu = cpu_util(
            m.map(|m| m.cpu_millicores),
            p.cpu_request_millis.map(|x| x as u64),
        );
        let mem = mem_util(m.map(|m| m.mem_mib), p.mem_request_mib.map(|x| x as u64));
        let mut cells: Vec<String> = Vec::new();
        if all_ns {
            cells.push(p.namespace.clone());
        }
        cells.push(p.name.clone());
        let pf = if self.forwarded.contains(&p.key) {
            "\u{25cf}"
        } else {
            ""
        };
        cells.push(pf.to_string()); // PF column: dot when a forward is live
        cells.push(p.ready.clone());
        cells.push(p.status.clone());
        cells.push(p.restarts.to_string());
        cells.push(cpu.text);
        cells.push(mem.text);
        cells.push(p.ip.clone().unwrap_or_default());
        cells.push(p.node.clone().unwrap_or_default());
        cells.push(kxs_core::format::age(p.created.as_deref(), now_ms));
        cells
    }

    /// Column widths per `columns()`; NAME absorbs slack, AGE kept last.
    fn layout(&self, total: u16, all_ns: bool, rows: &[PodRow], now_ms: i64) -> Vec<u16> {
        if rows.is_empty() {
            return vec![];
        }
        let first = self.cells(all_ns, &rows[0], now_ms);
        let mut widths: Vec<u16> = first
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let header = self.columns(all_ns)[i].chars().count() as u16;
                let widest = rows
                    .iter()
                    .map(|r| {
                        self.cells(all_ns, r, now_ms)
                            .get(i)
                            .map(|c| c.chars().count() as u16)
                            .unwrap_or(0)
                    })
                    .max()
                    .unwrap_or(0);
                header.max(widest).min(60)
            })
            .collect();
        if widths.is_empty() {
            return widths;
        }
        let overhead = |n: usize| 2u16 + (n as u16).saturating_sub(1);
        let age_idx = widths.len() - 1;
        while !self.wide && widths.len() > 2 {
            let n = widths.len();
            let needed: u16 = widths.iter().sum::<u16>() + overhead(n);
            if needed <= total {
                break;
            }
            // drop from the right, never NAME (0) or AGE (last)
            let drop_at = (n - 2).max(1);
            if drop_at == 0 {
                break;
            }
            widths.remove(drop_at);
        }
        if widths.len() > 2 {
            let n = widths.len();
            let others: u16 = widths[1..].iter().sum();
            let avail = total.saturating_sub(overhead(n));
            widths[0] = avail.saturating_sub(others).max(4);
        }
        let _ = age_idx;
        widths
    }

    fn visible_rows(&self) -> Vec<PodRow> {
        let pred = kxs_cluster::table::filter_predicate(&self.name_filter);
        let base: Vec<PodRow> = self
            .rows
            .iter()
            .filter(|p| pred(&p.name))
            .filter(|p| !self.faults_only || is_faulty(p))
            .cloned()
            .collect();
        let Some((col, dir)) = self.sort else {
            return base;
        };
        match col {
            SortCol::Cpu | SortCol::Mem => {
                let mem = col == SortCol::Mem;
                let mut rows = base;
                rows.sort_by(|x, y| {
                    let get = |r: &PodRow| -> Option<u64> {
                        let m = self.metrics.get(&r.key)?;
                        Some(if mem { m.mem_mib } else { m.cpu_millicores })
                    };
                    match (get(x), get(y)) {
                        (None, None) => std::cmp::Ordering::Equal,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (Some(a), Some(b)) => {
                            let ord = a.cmp(&b);
                            if dir == kxs_cluster::table::SortDir::Desc {
                                ord.reverse()
                            } else {
                                ord
                            }
                        }
                    }
                });
                rows
            }
            other => {
                let field = match other {
                    SortCol::Name => PodField::Name,
                    SortCol::Ready => PodField::Ready,
                    SortCol::Status => PodField::Status,
                    SortCol::Restarts => PodField::Restarts,
                    SortCol::Ip => PodField::Ip,
                    SortCol::Node => PodField::Node,
                    SortCol::Age => PodField::Age,
                    SortCol::Cpu | SortCol::Mem => unreachable!(),
                };
                sort_pods(&base, field, dir)
            }
        }
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

    fn sort_key_for(&self, c: char) -> Option<SortCol> {
        match c {
            'N' => Some(SortCol::Name),
            'A' => Some(SortCol::Age),
            'S' => Some(SortCol::Status),
            'C' => Some(SortCol::Cpu),
            'M' => Some(SortCol::Mem),
            'R' => Some(SortCol::Restarts),
            _ => None,
        }
    }
}

fn status_style(status: &str, th: &Theme) -> Option<Style> {
    let s = status.trim();
    if s == "Running" {
        Some(Style::new().fg(th.colors.green))
    } else if s == "Pending" || s.starts_with("ContainerCreating") || s.starts_with("Terminating") {
        Some(Style::new().fg(th.colors.yellow))
    } else if s.starts_with("CrashLoopBackOff")
        || s.starts_with("Error")
        || s.starts_with("Failed")
        || s.starts_with("Evicted")
        || s.starts_with("ImagePullBackOff")
    {
        Some(Style::new().fg(th.colors.red))
    } else {
        None
    }
}

impl View for PodsView {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        let ns = match &self.watched_ns {
            Some(n) => n.as_str(),
            None => "all",
        };
        let mut title = format!("Pods({})[{}]", ns, self.visible_rows().len());
        if !self.filter.is_empty() {
            title.push_str(&format!("  filter: {}", self.filter));
        }
        if let Some(status) = &self.status {
            title.push_str(&format!("  {status}"));
        }
        title
    }

    fn crumb(&self) -> String {
        "pods".into()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![
            Hint::action("enter", "containers"),
            Hint::action("l", "logs"),
            Hint::action("shift-l", "logs all"),
            Hint::action("d", "describe"),
            Hint::action("y", "yaml"),
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
            KeyCode::Char(c) if c.is_ascii_uppercase() => {
                let Some(field) = self.sort_key_for(c) else {
                    return vec![];
                };
                // cycle asc → desc → none
                self.sort = match self.sort {
                    Some((f, d)) if f == field => match d {
                        kxs_cluster::table::SortDir::Asc => {
                            Some((field, kxs_cluster::table::SortDir::Desc))
                        }
                        kxs_cluster::table::SortDir::Desc => None,
                    },
                    _ => Some((field, kxs_cluster::table::SortDir::Asc)),
                };
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
            crate::msg::Msg::Tick => {
                self.forwarded = ctx.forwards.iter().cloned().collect();
                if self.selector_pending {
                    return vec![];
                }
                if self.handle.is_none() && !self.pending {
                    return vec![self.restart_watch()];
                }
                if self.handle.is_some() && ctx.namespace != self.watched_ns {
                    self.watched_ns = ctx.namespace.clone();
                    let mut cmds = self.stop_old();
                    cmds.push(self.restart_watch());
                    return cmds;
                }
                vec![]
            }
            crate::msg::Msg::Pod { ev, .. } => {
                use kxs_cluster::pods::PodEvent::*;
                match ev {
                    Snapshot { rows } => {
                        self.rows = rows.clone();
                        self.status = None;
                        self.loaded = true;
                        self.fix_selection();
                    }
                    Upsert { rows } => {
                        for r in rows {
                            match self.rows.iter_mut().find(|x| x.key == r.key) {
                                Some(slot) => *slot = r.clone(),
                                None => self.rows.push(r.clone()),
                            }
                        }
                        self.fix_selection();
                    }
                    Delete { keys } => {
                        self.rows.retain(|r| !keys.contains(&r.key));
                        self.fix_selection();
                    }
                    Status { state, message } => {
                        self.status = crate::view::status_suffix(state, message.as_deref());
                    }
                }
                vec![]
            }
            crate::msg::Msg::Metrics { pods, .. } => {
                if let Ok(rows) = pods {
                    for m in rows {
                        self.metrics.insert(m.key.clone(), m.clone());
                    }
                }
                vec![]
            }
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
            crate::msg::Msg::Fetched {
                result: Ok(crate::cmd::FetchResult::Selector(s)),
                ..
            } => {
                self.selector = Some(s.clone());
                self.selector_pending = false;
                self.watched_ns = ctx.namespace.clone();
                vec![self.restart_watch()]
            }
            _ => vec![],
        }
    }

    fn set_filter(&mut self, filter: &str) -> Vec<Cmd> {
        self.filter = filter.to_string();
        let (labels, name) = kxs_cluster::table::split_filter(filter);
        let selector_changed = labels != self.filter_selector;
        self.filter_selector = labels;
        self.name_filter = name;
        self.fix_selection();
        if selector_changed {
            let mut cmds = self.stop_old();
            cmds.push(self.restart_watch());
            self.rows.clear();
            return cmds;
        }
        vec![]
    }

    fn filter(&self) -> String {
        self.filter.clone()
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

    fn target(&self) -> Option<Target> {
        let rows = self.visible_rows();
        let row = rows
            .iter()
            .find(|r| Some(r.key.as_str()) == self.selected.as_deref())?;
        Some(Target {
            kind: pod_kind_from(row),
            ns: Some(row.namespace.clone()),
            name: row.name.clone(),
            container: None,
            desired_replicas: None,
            suspend: None,
            unschedulable: None,
        })
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, _filter: &str) {
        self.viewport_rows.set(area.height.saturating_sub(2));
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(th.colors.border))
            .title(Line::from(Span::styled(
                self.title(),
                Style::new().fg(th.colors.accent),
            )));
        if self.rows.is_empty() {
            f.render_widget(block, area);
            let msg = if let Some(status) = &self.status {
                status.clone()
            } else if !self.loaded {
                "loading…".into()
            } else {
                "no pods".into()
            };
            f.render_widget(
                Paragraph::new(msg).style(Style::new().fg(th.colors.fg_dim)),
                Rect {
                    x: area.x + 2,
                    y: area.y + 1,
                    width: area.width.saturating_sub(4),
                    height: 1,
                },
            );
            return;
        }
        let all_ns = self.watched_ns.is_none();
        let now_ms = kxs_cluster::clock::now_ms();
        let rows_sorted = self.visible_rows();
        let widths = self.layout(area.width, all_ns, &rows_sorted, now_ms);
        let cols = self.columns(all_ns);
        let header_cells: Vec<Span> = widths
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let indicator = match sort_field_index(i, all_ns) {
                    Some(field) => sort_indicator_of(self.sort, field),
                    None => String::new(),
                };
                Span::styled(
                    format!("{}{}", cols[i], indicator),
                    Style::new().fg(th.colors.fg_dim).bold(),
                )
            })
            .collect();
        let rows = rows_sorted.iter().map(|p| {
            let cells = self.cells(all_ns, p, now_ms);
            let spans: Vec<Span> = widths
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    let text = truncate_cell(cells.get(i).map(String::as_str).unwrap_or(""), *w);
                    let style = if cols[i] == "STATUS" {
                        status_style(&p.status, th)
                    } else {
                        None
                    };
                    Span::styled(text, style.unwrap_or_else(|| Style::new().fg(th.colors.fg)))
                })
                .collect();
            Row::new(spans).style(if self.selected.as_deref() == Some(p.key.as_str()) {
                Style::new().bg(th.colors.bg_active)
            } else {
                Style::new()
            })
        });
        let constraints: Vec<Constraint> = widths.iter().map(|w| Constraint::Length(*w)).collect();
        let table = Table::new(rows, constraints)
            .header(Row::new(header_cells))
            .block(block);
        self.scroll.render(f, area, table, self.selected_index());
    }
}

fn sort_field_index(display: usize, all_ns: bool) -> Option<SortCol> {
    // columns: [NAMESPACE] NAME PF READY STATUS RESTARTS CPU MEM IP NODE AGE
    let idx = if all_ns {
        display as i32 - 1
    } else {
        display as i32
    };
    match idx {
        0 => Some(SortCol::Name),
        2 => Some(SortCol::Ready),
        3 => Some(SortCol::Status),
        4 => Some(SortCol::Restarts),
        5 => Some(SortCol::Cpu),
        6 => Some(SortCol::Mem),
        7 => Some(SortCol::Ip),
        8 => Some(SortCol::Node),
        9 => Some(SortCol::Age),
        _ => None,
    }
}

fn sort_indicator_of(
    sort: Option<(SortCol, kxs_cluster::table::SortDir)>,
    field: SortCol,
) -> String {
    match sort {
        Some((f, kxs_cluster::table::SortDir::Asc)) if f == field => "▲".into(),
        Some((f, kxs_cluster::table::SortDir::Desc)) if f == field => "▼".into(),
        _ => String::new(),
    }
}

fn truncate_cell(s: &str, max: u16) -> String {
    let max = max as usize;
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

fn pod_kind_from(_row: &PodRow) -> ResourceKind {
    ResourceKind {
        group: String::new(),
        version: "v1".into(),
        kind: "Pod".into(),
        plural: "pods".into(),
        namespaced: true,
        aliases: vec!["po".into()],
    }
}

/// ANDs a drill-down selector with a `-l` filter selector.
fn join_selectors(a: Option<&str>, b: Option<&str>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => Some(format!("{a},{b}")),
        (Some(a), None) => Some(a.to_string()),
        (None, Some(b)) => Some(b.to_string()),
        (None, None) => None,
    }
}

/// `ctrl-z` predicate: anything a human would want to look at.
fn is_faulty(p: &PodRow) -> bool {
    if p.restarts > 0 {
        return true;
    }
    match p.status.as_str() {
        "Running" | "Succeeded" | "Completed" => !ready_matches(p),
        _ => true,
    }
}

/// "2/2" — all containers up.
fn ready_matches(p: &PodRow) -> bool {
    match p.ready.split_once('/') {
        Some((a, b)) => a == b,
        None => true,
    }
}
