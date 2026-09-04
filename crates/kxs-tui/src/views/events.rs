//! Events view: server-side Event table, newest first, Warning rows red.

use std::cell::Cell;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use kxs_cluster::discovery::ResourceKind;
use kxs_cluster::events::{
    column_index, event_filter_text, event_type_weight, sort_events_newest_first, Weight,
};
use kxs_cluster::resources::{ResourceTable, TableEvent};

use crate::cmd::{Cmd, StopHandle};
use crate::select::move_selection;
use crate::theme::Theme;
use crate::view::{Hint, View};
use crate::AppCtx;

pub struct EventsView {
    id: u64,
    watched_ns: Option<String>,
    filter: String,
    table: Option<ResourceTable>,
    selected: Option<String>,
    handle: Option<StopHandle>,
    pending: bool,
    status: Option<String>,
    viewport_rows: Cell<u16>,
    scroll: crate::table::Scroll,
}

impl EventsView {
    pub fn new(app: &mut crate::app::App, ns: Option<String>) -> Self {
        EventsView {
            id: app.alloc_id(),
            watched_ns: ns,
            filter: String::new(),
            table: None,
            selected: None,
            handle: None,
            pending: false,
            status: None,
            viewport_rows: Cell::new(20),
            scroll: Default::default(),
        }
    }

    fn restart_watch(&mut self) -> Cmd {
        self.pending = true;
        Cmd::StartTableWatch {
            view: self.id,
            kind: events_kind(),
            ns: self.watched_ns.clone(),
            selector: None,
        }
    }

    fn stop_old(&mut self) -> Vec<Cmd> {
        match self.handle.take() {
            Some(h) => vec![Cmd::Stop(h)],
            None => vec![],
        }
    }

    /// Rows sorted newest first (Last Seen column for fallback ordering).
    fn ordered(&self) -> Vec<kxs_cluster::resources::ResourceRow> {
        let Some(t) = &self.table else { return vec![] };
        let last_seen = column_index(&t.columns, "Last Seen");
        sort_events_newest_first(&t.rows, last_seen, kxs_cluster::clock::now_ms())
    }

    fn visible(&self) -> Vec<kxs_cluster::resources::ResourceRow> {
        let rows = self.ordered();
        if self.filter.is_empty() {
            return rows;
        }
        let Some(t) = &self.table else { return rows };
        let reason = column_index(&t.columns, "Reason");
        let object = column_index(&t.columns, "Object");
        let message = column_index(&t.columns, "Message");
        let pred = kxs_cluster::table::filter_predicate(&self.filter);
        rows.into_iter()
            .filter(|r| pred(&event_filter_text(r, &[reason, object, message])))
            .collect()
    }

    fn keys(&self) -> Vec<String> {
        self.visible().iter().map(|r| r.key.clone()).collect()
    }

    fn selected_index(&self) -> Option<usize> {
        let sel = self.selected.as_deref()?;
        self.visible().iter().position(|r| r.key == sel)
    }
}

pub fn events_kind() -> ResourceKind {
    ResourceKind {
        group: String::new(),
        version: "v1".into(),
        kind: "Event".into(),
        plural: "events".into(),
        namespaced: true,
        aliases: vec!["ev".into()],
    }
}

impl View for EventsView {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        let ns = match &self.watched_ns {
            Some(n) => n.as_str(),
            None => "all",
        };
        let mut title = format!("Events({})[{}]", ns, self.visible().len());
        if let Some(status) = &self.status {
            title.push_str(&format!("  {status}"));
        }
        title
    }

    fn crumb(&self) -> String {
        "events".into()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![Hint::action("ctrl-r", "refresh")]
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
            crate::msg::Msg::Table { ev, .. } => match ev {
                TableEvent::Table { table } => {
                    self.status = None;
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
                    self.status = crate::view::status_suffix(state, message.as_deref());
                    vec![]
                }
            },
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

    fn on_pop(&mut self) -> Vec<Cmd> {
        self.stop_old()
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
        let Some(t) = &self.table else {
            f.render_widget(block, area);
            f.render_widget(
                Paragraph::new("loading…").style(Style::new().fg(th.colors.fg_dim)),
                Rect {
                    x: area.x + 2,
                    y: area.y + 1,
                    width: area.width.saturating_sub(4),
                    height: 1,
                },
            );
            return;
        };
        let visible = self.visible();
        let type_col = column_index(&t.columns, "Type");
        let widths: Vec<u16> = t
            .columns
            .iter()
            .map(|c| (c.chars().count() as u16).max(6))
            .collect();
        let now_ms = kxs_cluster::clock::now_ms();
        let age_col = column_index(&t.columns, "Age");
        let rows = visible.iter().map(|r| {
            let spans: Vec<Span> = t
                .columns
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    let text = if i as i32 == age_col {
                        kxs_core::format::age(r.created.as_deref(), now_ms)
                    } else {
                        r.cells.get(i).cloned().unwrap_or_default()
                    };
                    let style = if i as i32 == type_col {
                        match event_type_weight(&text) {
                            Weight::Bad => Style::new().fg(th.colors.red),
                            Weight::Warn => Style::new().fg(th.colors.yellow),
                            Weight::Dim => Style::new().fg(th.colors.fg_dim),
                            Weight::None => Style::new().fg(th.colors.fg),
                        }
                    } else {
                        Style::new().fg(th.colors.fg)
                    };
                    Span::styled(text, style)
                })
                .collect();
            Row::new(spans).style(if self.selected.as_deref() == Some(r.key.as_str()) {
                Style::new().bg(th.colors.bg_active)
            } else {
                Style::new()
            })
        });
        let constraints: Vec<Constraint> = widths.iter().map(|w| Constraint::Length(*w)).collect();
        let table = Table::new(rows, constraints)
            .header(Row::new(t.columns.clone()).style(Style::new().fg(th.colors.fg_dim).bold()))
            .block(block);
        self.scroll.render(f, area, table, self.selected_index());
    }
}
