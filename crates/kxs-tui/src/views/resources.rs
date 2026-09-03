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

use crate::clipboard;
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
    table: Option<ResourceTable>,
    /// Sort key is the original cell index; `cells.len()` is the synthetic AGE column.
    sort: Option<Sort>,
    selected: Option<String>,
    handle: Option<StopHandle>,
    status: Option<String>,
    viewport_rows: Cell<u16>,
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
            table: None,
            sort: None,
            selected: None,
            handle: None,
            status: None,
            viewport_rows: Cell::new(20),
        }
    }

    /// Display order of original cell indexes, AGE forced last.
    fn column_map(&self) -> Vec<usize> {
        let Some(t) = &self.table else { return vec![] };
        let mut idx: Vec<usize> = (0..t.columns.len()).collect();
        if let Some(i) = t
            .columns
            .iter()
            .position(|c| c.trim().eq_ignore_ascii_case("age"))
        {
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
            Some(&ci) if t.columns[ci].trim().eq_ignore_ascii_case("age") => t.columns.len(),
            Some(&ci) => ci,
            None => display,
        }
    }

    /// Display columns as (original cell index, width). Widths are
    /// max(header, widest cell) capped at 60. When the sum exceeds the area,
    /// rightmost non-NAME non-AGE columns are dropped; NAME absorbs the
    /// remaining slack. AGE is always kept last.
    fn layout(&self, total: u16) -> Vec<(usize, u16)> {
        let Some(t) = &self.table else { return vec![] };
        let map = self.column_map();
        let mut cols: Vec<(usize, u16)> = map
            .iter()
            .map(|&ci| {
                let hdr = t.columns[ci].chars().count() as u16;
                let widest = t
                    .rows
                    .iter()
                    .map(|r| r.cells.get(ci).map_or(0, |c| c.chars().count() as u16))
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
        while cols.len() > 2 {
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
        let rows: Vec<ResourceRow> = t.rows.iter().filter(|r| pred(&r.name)).cloned().collect();
        match self.sort {
            Some(s) => sort_rows(&rows, s.key, s.dir),
            None => rows,
        }
    }

    fn keys(&self) -> Vec<String> {
        self.visible_rows().iter().map(|r| r.key.clone()).collect()
    }

    fn target_of(&self, r: &ResourceRow) -> crate::view::Target {
        crate::view::Target {
            kind: self.kind.clone(),
            ns: self.watched_ns.clone(),
            name: r.name.clone(),
        }
    }

    fn restart_watch(&self) -> Cmd {
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
            Hint {
                key: "d",
                desc: "describe",
            },
            Hint {
                key: "y",
                desc: "yaml",
            },
            Hint {
                key: "r",
                desc: "refresh",
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
                    clipboard::copy(sel);
                }
                vec![]
            }
            KeyCode::Char('r') => {
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
        self.status = None;
        vec![]
    }

    fn on_msg(&mut self, msg: &crate::msg::Msg, ctx: &AppCtx) -> Vec<Cmd> {
        match msg {
            crate::msg::Msg::Tick => {
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
                    // selection survives re-sorts and watch updates by key
                    if self
                        .selected
                        .as_ref()
                        .is_none_or(|sel| !table.rows.iter().any(|r| &r.key == sel))
                    {
                        self.selected = table.rows.first().map(|r| r.key.clone());
                    }
                    self.table = Some(table.clone());
                    vec![]
                }
                TableEvent::Status { state, message } => {
                    self.status = Some(match (state.as_str(), message) {
                        ("connected", _) => "⟳ connected".into(),
                        ("error", Some(m)) => format!("⟳ {m}"),
                        _ => "⟳ reconnecting".into(),
                    });
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
        let cols = self.layout(area.width);
        let header_cells: Vec<Span> = cols
            .iter()
            .map(|(ci, _)| {
                let t = self.table.as_ref().expect("table");
                let key = if t.columns[*ci].trim().eq_ignore_ascii_case("age") {
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
                    let text = r.cells.get(*ci).cloned().unwrap_or_default();
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
        f.render_widget(table, area);
    }
}
