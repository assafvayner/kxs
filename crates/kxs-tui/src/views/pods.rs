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
    /// `/` free-text filter on the pod name.
    filter: String,
    rows: Vec<PodRow>,
    /// key → latest metrics
    metrics: HashMap<String, MetricsRow>,
    sort: Option<(SortCol, kxs_cluster::table::SortDir)>,
    selected: Option<String>,
    handle: Option<StopHandle>,
    pending: bool,
    status: Option<String>,
    viewport_rows: Cell<u16>,
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
            rows: vec![],
            metrics: HashMap::new(),
            sort: None,
            selected: None,
            handle: None,
            pending: false,
            status: None,
            viewport_rows: Cell::new(20),
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

    fn restart_watch(&mut self) -> Cmd {
        self.pending = true;
        Cmd::StartPodWatch {
            view: self.id,
            ns: self.watched_ns.clone(),
            selector: self.selector.clone(),
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
        cells.push(String::new()); // PF, filled by render from the forwards registry
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
        while widths.len() > 2 {
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
        let base: Vec<PodRow> = if self.filter.is_empty() {
            self.rows.clone()
        } else {
            let needle = self.filter.to_lowercase();
            self.rows
                .iter()
                .filter(|p| p.name.to_lowercase().contains(&needle))
                .cloned()
                .collect()
        };
        let Some((col, dir)) = self.sort else {
            return base;
        };
        match col {
            SortCol::Cpu | SortCol::Mem => {
                let mem = col == SortCol::Mem;
                let mut rows = self.rows.clone();
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
            Hint {
                key: "enter",
                desc: "containers",
            },
            Hint {
                key: "l",
                desc: "logs",
            },
            Hint {
                key: "shift-l",
                desc: "logs all",
            },
            Hint {
                key: "d",
                desc: "describe",
            },
            Hint {
                key: "y",
                desc: "yaml",
            },
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
            KeyCode::Char('c') => {
                if let Some(sel) = &self.selected {
                    crate::clipboard::copy(sel);
                }
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
        self.status = None;
        vec![]
    }

    fn on_msg(&mut self, msg: &crate::msg::Msg, ctx: &AppCtx) -> Vec<Cmd> {
        match msg {
            crate::msg::Msg::Tick => {
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
                        if self.selected.is_none() {
                            self.selected = self.rows.first().map(|r| r.key.clone());
                        }
                    }
                    Upsert { rows } => {
                        for r in rows {
                            match self.rows.iter_mut().find(|x| x.key == r.key) {
                                Some(slot) => *slot = r.clone(),
                                None => self.rows.push(r.clone()),
                            }
                        }
                    }
                    Delete { keys } => {
                        self.rows.retain(|r| !keys.contains(&r.key));
                        if let Some(sel) = &self.selected {
                            if keys.contains(sel) {
                                self.selected = self.rows.first().map(|r| r.key.clone());
                            }
                        }
                    }
                    Status { state, message } => {
                        self.status = Some(match (state.as_str(), message) {
                            ("connected", _) => "⟳ connected".into(),
                            ("error", Some(m)) => format!("⟳ {m}"),
                            _ => "⟳ reconnecting".into(),
                        });
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
        // keep the selection if it survives the filter, else take the first visible
        let keys = self.keys();
        if !keys
            .iter()
            .any(|k| Some(k.as_str()) == self.selected.as_deref())
        {
            self.selected = keys.first().cloned();
        }
        vec![]
    }

    fn filter(&self) -> String {
        self.filter.clone()
    }

    fn on_pop(&mut self) -> Vec<Cmd> {
        self.stop_old()
    }

    fn target(&self) -> Option<Target> {
        let row = self
            .rows
            .iter()
            .find(|r| Some(r.key.as_str()) == self.selected.as_deref())?;
        Some(Target {
            kind: pod_kind_from(row),
            ns: Some(row.namespace.clone()),
            name: row.name.clone(),
            container: None,
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
            let msg = if self.status.is_some() {
                self.status.clone().unwrap_or_default()
            } else {
                "loading…".into()
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
        f.render_widget(
            Table::new(rows, constraints)
                .header(Row::new(header_cells))
                .block(block),
            area,
        );
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
