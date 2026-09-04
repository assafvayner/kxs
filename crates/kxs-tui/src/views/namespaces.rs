//! Namespace switching.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use crate::cmd::{Cmd, Fetch};
use crate::theme::Theme;
use crate::view::{Hint, View};
use crate::AppCtx;

pub struct NamespacesView {
    id: u64,
    /// `None` = the "all" row.
    namespaces: Vec<Option<String>>,
    selected: usize,
    loaded: bool,
    in_flight: bool,
    error: Option<String>,
}

impl NamespacesView {
    pub fn new(app: &mut crate::app::App) -> Self {
        NamespacesView {
            id: app.alloc_id(),
            namespaces: vec![None],
            selected: 0,
            loaded: false,
            in_flight: false,
            error: None,
        }
    }
}

impl View for NamespacesView {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        format!("Namespaces[{}]", self.namespaces.len())
    }

    fn crumb(&self) -> String {
        "ns".into()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![Hint::action("enter", "switch")]
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        let last = self.namespaces.len().saturating_sub(1);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Home | KeyCode::Char('g') => self.selected = 0,
            KeyCode::End | KeyCode::Char('G') => self.selected = last,
            KeyCode::Enter => {
                return match self.namespaces.get(self.selected) {
                    Some(ns) => {
                        vec![Cmd::SwitchNamespace { ns: ns.clone() }]
                    }
                    None => vec![],
                };
            }
            _ => {}
        }
        vec![]
    }

    fn on_msg(&mut self, msg: &crate::msg::Msg, _ctx: &AppCtx) -> Vec<Cmd> {
        match msg {
            crate::msg::Msg::Tick if !self.loaded && !self.in_flight => {
                self.in_flight = true;
                vec![Cmd::Fetch {
                    view: self.id,
                    what: Fetch::Namespaces,
                }]
            }
            crate::msg::Msg::Fetched {
                result: Ok(crate::cmd::FetchResult::Namespaces(list)),
                ..
            } => {
                self.loaded = true;
                self.in_flight = false;
                let mut ns: Vec<Option<String>> = vec![None];
                ns.extend(list.iter().cloned().map(Some));
                self.namespaces = ns;
                vec![]
            }
            crate::msg::Msg::Fetched { result: Err(e), .. } => {
                self.loaded = true;
                self.in_flight = false;
                self.error = Some(e.clone());
                vec![]
            }
            _ => vec![],
        }
    }

    fn wants_filter(&self) -> bool {
        false
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, _filter: &str) {
        let rows = self.namespaces.iter().enumerate().map(|(i, ns)| {
            let name = ns.clone().unwrap_or_else(|| "all".into());
            Row::new(vec![Span::raw(name)]).style(if i == self.selected {
                Style::new().bg(th.colors.bg_active)
            } else {
                Style::new()
            })
        });
        let table = Table::new(rows, [ratatui::layout::Constraint::Percentage(100)])
            .header(Row::new(["NAME"]).style(Style::new().fg(th.colors.fg_dim).bold()))
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .border_style(Style::new().fg(th.colors.border))
                    .title(Line::from(Span::styled(
                        self.title(),
                        Style::new().fg(th.colors.accent),
                    ))),
            );
        f.render_widget(table, area);
        if self.namespaces.len() <= 1 {
            let loading = Paragraph::new(self.error.as_deref().unwrap_or("loading…"))
                .style(Style::new().fg(th.colors.fg_dim));
            f.render_widget(loading, area);
        }
    }
}
