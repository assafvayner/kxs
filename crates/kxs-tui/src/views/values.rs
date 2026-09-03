//! Values view for ConfigMap/Secret data: keys pane + value pane.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use kxs_cluster::workloads::ConfigEntry;

use crate::clipboard;
use crate::cmd::{Cmd, StopHandle};
use crate::theme::Theme;
use crate::view::{Hint, Target, View};
use crate::AppCtx;

pub struct ValuesView {
    id: u64,
    target: Target,
    entries: Vec<ConfigEntry>,
    selected: usize,
    /// Secret values masked until `x` is pressed.
    masked: bool,
    handle: Option<StopHandle>,
    pending: bool,
    error: Option<String>,
}

impl ValuesView {
    pub fn new(app: &mut crate::app::App, target: Target) -> Self {
        ValuesView {
            id: app.alloc_id(),
            target: target.clone(),
            entries: vec![],
            selected: 0,
            masked: target.kind.kind == "Secret",
            handle: None,
            pending: true,
            error: None,
        }
    }

    fn current(&self) -> Option<&ConfigEntry> {
        self.entries.get(self.selected)
    }

    fn display_value(&self, e: &ConfigEntry) -> String {
        if self.masked {
            "•••••••• (x to reveal)".into()
        } else if e.binary {
            format!("{}\n(base64, binary)", e.value)
        } else {
            e.value.clone()
        }
    }
}

impl View for ValuesView {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        format!(
            "Values({}/{})[{}]",
            self.target.kind.kind,
            self.target.name,
            self.entries.len()
        )
    }

    fn crumb(&self) -> String {
        "values".into()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![
            Hint::action("x", "toggle mask"),
            Hint::action("c", "copy value"),
        ]
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        let last = self.entries.len().saturating_sub(1);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Char('x') => self.masked = !self.masked,
            KeyCode::Char('c') => {
                // masked secrets never leave via copy; reveal first (x)
                if let Some(e) = self.current().filter(|_| !self.masked) {
                    clipboard::copy(&e.value);
                }
            }
            _ => {}
        }
        vec![]
    }

    fn on_started(&mut self, handle: StopHandle, _ctx: &AppCtx) -> Vec<Cmd> {
        self.handle = Some(handle);
        vec![]
    }

    fn on_msg(&mut self, msg: &crate::msg::Msg, _ctx: &AppCtx) -> Vec<Cmd> {
        match msg {
            crate::msg::Msg::Tick if self.pending && self.error.is_none() => {
                vec![Cmd::Fetch {
                    view: self.id,
                    what: crate::cmd::Fetch::ConfigValues {
                        ns: self.target.ns.clone().unwrap_or_default(),
                        name: self.target.name.clone(),
                        kind: self.target.kind.kind.clone(),
                    },
                }]
            }
            crate::msg::Msg::Fetched { view, result } if *view == self.id => {
                self.pending = false;
                match result {
                    Ok(crate::cmd::FetchResult::Values(entries)) => self.entries = entries.clone(),
                    Err(e) => self.error = Some(e.clone()),
                    _ => {}
                }
                vec![]
            }
            _ => vec![],
        }
    }

    fn wants_filter(&self) -> bool {
        false
    }

    fn on_pop(&mut self) -> Vec<Cmd> {
        match self.handle.take() {
            Some(h) => vec![Cmd::Stop(h)],
            None => vec![],
        }
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, _filter: &str) {
        let [keys_area, value_area] =
            Layout::vertical([Constraint::Percentage(35), Constraint::Percentage(65)]).areas(area);
        let block = |title: String| {
            Block::new()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(th.colors.border))
                .title(Line::from(Span::styled(
                    title,
                    Style::new().fg(th.colors.accent),
                )))
        };
        if self.entries.is_empty() {
            f.render_widget(block(self.title()), area);
            let msg = self.error.clone().unwrap_or_else(|| "loading…".into());
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
        // keys pane
        let mut key_lines: Vec<Line> = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            key_lines.push(Line::from(Span::styled(
                e.key.clone(),
                if i == self.selected {
                    Style::new().bg(th.colors.bg_active).fg(th.colors.fg)
                } else {
                    Style::new().fg(th.colors.fg)
                },
            )));
        }
        f.render_widget(
            Paragraph::new(key_lines).block(block("Keys".into())),
            keys_area,
        );
        // value pane
        let value_text = self
            .current()
            .map(|e| self.display_value(e))
            .unwrap_or_default();
        f.render_widget(
            Paragraph::new(value_text).block(block("Value".into())),
            value_area,
        );
    }
}
