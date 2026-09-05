//! Contexts view: kubeconfig rows with background reachability pings.

use std::collections::{HashMap, HashSet};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use kxs_core::kubeconfig::store::KubeconfigStore;

use crate::cmd::Cmd;
use crate::select::move_selection;
use crate::theme::Theme;
use crate::view::{Hint, View};
use crate::AppCtx;

const MAX_PINGS_IN_FLIGHT: usize = 4;

pub struct ContextsView {
    id: u64,
    rows: Vec<Row0>,
    selected: Option<String>,
    /// context name → ping result (version or error)
    pings: HashMap<String, Result<String, String>>,
    pending: HashSet<String>,
    filter: String,
    scroll: crate::table::Scroll,
    /// Body height of the last frame, for page-sized moves.
    viewport_rows: std::cell::Cell<u16>,
}

struct Row0 {
    name: String,
    cluster: String,
    user: String,
    namespace: Option<String>,
}

impl ContextsView {
    pub fn new(app: &mut crate::app::App) -> Self {
        let rows = context_rows(&app.sessions.lock().expect("sessions lock").store);
        let selected = rows.first().map(|r| r.name.clone());
        ContextsView {
            id: app.alloc_id(),
            rows,
            selected,
            pings: HashMap::new(),
            pending: HashSet::new(),
            filter: String::new(),
            scroll: Default::default(),
            viewport_rows: std::cell::Cell::new(10),
        }
    }

    /// Rows surviving the `/` filter, in display order.
    fn visible_rows(&self) -> Vec<&Row0> {
        let pred = kxs_cluster::table::filter_predicate(&self.filter);
        self.rows
            .iter()
            .filter(|r| pred(&format!("{} {}", r.name, r.cluster)))
            .collect()
    }

    fn keys(&self) -> Vec<String> {
        self.visible_rows().iter().map(|r| r.name.clone()).collect()
    }

    fn selected_index(&self) -> Option<usize> {
        let sel = self.selected.as_deref()?;
        self.visible_rows().iter().position(|r| r.name == sel)
    }

    fn page(&self) -> isize {
        self.viewport_rows.get().max(1) as isize
    }

    /// One ping per not-yet-answered context, requested once.
    fn ping_missing(&mut self) -> Vec<Cmd> {
        let mut cmds = vec![];
        for name in self.rows.iter().map(|r| r.name.clone()) {
            if self.pending.len() >= MAX_PINGS_IN_FLIGHT {
                break;
            }
            if !self.pings.contains_key(&name) && !self.pending.contains(&name) {
                self.pending.insert(name.clone());
                cmds.push(Cmd::Ping { context: name });
            }
        }
        cmds
    }
}

fn context_rows(store: &KubeconfigStore) -> Vec<Row0> {
    store
        .contexts()
        .into_iter()
        .map(|c| Row0 {
            name: c.name,
            cluster: c.cluster,
            user: c.user,
            namespace: c.namespace,
        })
        .collect()
}

impl View for ContextsView {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        format!("Contexts[{}]", self.visible_rows().len())
    }

    fn crumb(&self) -> String {
        "ctx".into()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![
            Hint::action("enter", "connect"),
            Hint::action("/", "filter"),
            Hint::action("ctrl-r", "re-ping"),
        ]
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = move_selection(&self.keys(), self.selected.as_deref(), 1);
                vec![]
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = move_selection(&self.keys(), self.selected.as_deref(), -1);
                vec![]
            }
            KeyCode::PageDown => {
                let page = self.page();
                self.selected = move_selection(&self.keys(), self.selected.as_deref(), page);
                vec![]
            }
            KeyCode::PageUp => {
                let page = self.page();
                self.selected = move_selection(&self.keys(), self.selected.as_deref(), -page);
                vec![]
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let page = self.page();
                self.selected = move_selection(&self.keys(), self.selected.as_deref(), page);
                vec![]
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let page = self.page();
                self.selected = move_selection(&self.keys(), self.selected.as_deref(), -page);
                vec![]
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected = self.keys().first().cloned();
                vec![]
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected = self.keys().last().cloned();
                vec![]
            }
            KeyCode::Enter => match &self.selected {
                Some(name) => vec![Cmd::Connect {
                    context: name.clone(),
                }],
                None => vec![],
            },
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // re-ping everything
                self.pings.clear();
                self.pending.clear();
                self.ping_missing()
            }
            _ => vec![],
        }
    }

    fn on_msg(&mut self, msg: &crate::msg::Msg, _ctx: &AppCtx) -> Vec<Cmd> {
        match msg {
            crate::msg::Msg::Tick => self.ping_missing(),
            crate::msg::Msg::Pinged { context, result } => {
                self.pending.remove(context);
                self.pings.insert(context.clone(), result.clone());
                vec![]
            }
            _ => vec![],
        }
    }

    fn wants_filter(&self) -> bool {
        true
    }

    fn filter(&self) -> String {
        self.filter.clone()
    }

    fn set_filter(&mut self, filter: &str) -> Vec<Cmd> {
        self.filter = filter.to_string();
        let keys = self.keys();
        if !keys
            .iter()
            .any(|k| Some(k.as_str()) == self.selected.as_deref())
        {
            self.selected = keys.first().cloned();
        }
        vec![]
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, _filter: &str) {
        self.viewport_rows.set(area.height.saturating_sub(3));
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(th.colors.border))
            .title(Line::from(Span::styled(
                self.title(),
                Style::new().fg(th.colors.accent),
            )));
        f.render_widget(block, area);
        let visible = self.visible_rows();
        if visible.is_empty() {
            let msg = Paragraph::new(if self.rows.is_empty() {
                "no contexts in the kubeconfig"
            } else {
                "no contexts match the filter"
            })
            .style(Style::new().fg(th.colors.fg_dim));
            f.render_widget(
                msg,
                Rect {
                    x: area.x + 2,
                    y: area.y + 1,
                    width: area.width.saturating_sub(4),
                    height: 1,
                },
            );
            return;
        }
        let rows = visible.iter().map(|r| {
            let status = if let Some(res) = self.pings.get(&r.name) {
                match res {
                    Ok(v) => Span::styled(format!("✓ {v}"), Style::new().fg(th.colors.green)),
                    Err(e) => Span::styled(format!("✗ {e}"), Style::new().fg(th.colors.red)),
                }
            } else if self.pending.contains(&r.name) {
                Span::styled("…", Style::new().fg(th.colors.fg_dim))
            } else {
                Span::styled("·", Style::new().fg(th.colors.fg_dim))
            };
            Row::new(vec![
                Span::raw(r.name.clone()),
                Span::raw(r.cluster.clone()),
                Span::raw(r.user.clone()),
                Span::raw(r.namespace.clone().unwrap_or_default()),
                status,
            ])
            .style(if self.selected.as_deref() == Some(r.name.as_str()) {
                Style::new().bg(th.colors.bg_active)
            } else {
                Style::new()
            })
        });
        let widths = [
            ratatui::layout::Constraint::Percentage(25),
            ratatui::layout::Constraint::Percentage(30),
            ratatui::layout::Constraint::Percentage(20),
            ratatui::layout::Constraint::Percentage(10),
            ratatui::layout::Constraint::Min(8),
        ];
        let header = Row::new(["NAME", "CLUSTER", "USER", "NAMESPACE", "STATUS"])
            .style(Style::new().fg(th.colors.fg_dim).bold());
        let table = Table::new(rows, widths).header(header).block(
            Block::new()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(th.colors.border))
                .title(Line::from(Span::styled(
                    self.title(),
                    Style::new().fg(th.colors.accent),
                ))),
        );
        self.scroll.render(f, area, table, self.selected_index());
    }
}
