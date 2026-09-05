//! Forwards view: session-owned port-forwards; ctrl-d stops the selected one.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use kxs_core::format::age_secs;

use crate::cmd::Cmd;
use crate::theme::Theme;
use crate::view::{Hint, View};
use crate::AppCtx;

pub struct ForwardsView {
    id: u64,
    /// Snapshot of the session registry, refreshed each tick.
    forwards: Vec<(u64, String, String, u16, u16, i64)>, // id, ns, pod, pod_port, local_port, age_secs
    selected: usize,
    scroll: crate::table::Scroll,
}

impl ForwardsView {
    pub fn new(app: &mut crate::app::App) -> Self {
        ForwardsView {
            id: app.alloc_id(),
            forwards: vec![],
            selected: 0,
            scroll: Default::default(),
        }
    }
}

impl View for ForwardsView {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        format!("Forwards[{}]", self.forwards.len())
    }

    fn crumb(&self) -> String {
        "pf".into()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![Hint::mutation("ctrl-d", "stop")]
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        let last = self.forwards.len().saturating_sub(1);
        let mut cmds = vec![];
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some((id, ..)) = self.forwards.get(self.selected).cloned() {
                    cmds.push(Cmd::StopForward { id });
                }
            }
            _ => {}
        }
        cmds
    }

    fn wants_filter(&self) -> bool {
        false
    }

    fn on_msg(&mut self, msg: &crate::msg::Msg, ctx: &AppCtx) -> Vec<Cmd> {
        if let crate::msg::Msg::Tick = msg {
            // re-snapshot from the app-level registry mirror
            self.forwards = ctx
                .forward_rows
                .iter()
                .map(|(id, ns, pod, port, local, age)| {
                    (*id, ns.clone(), pod.clone(), *port, *local, *age)
                })
                .collect();
            let last = self.forwards.len().saturating_sub(1);
            self.selected = self.selected.min(last);
        }
        vec![]
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, _filter: &str) {
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(th.colors.border))
            .title(Line::from(Span::styled(
                self.title(),
                Style::new().fg(th.colors.accent),
            )));
        if self.forwards.is_empty() {
            f.render_widget(block, area);
            f.render_widget(
                Paragraph::new("no active port-forwards (shift-f on a pod)")
                    .style(Style::new().fg(th.colors.fg_dim)),
                Rect {
                    x: area.x + 2,
                    y: area.y + 1,
                    width: area.width.saturating_sub(4),
                    height: 1,
                },
            );
            return;
        }
        let rows = self
            .forwards
            .iter()
            .enumerate()
            .map(|(i, (id, ns, pod, port, local, age))| {
                let _ = id;
                Row::new(vec![
                    Span::styled(local.to_string(), Style::new().fg(th.colors.accent)),
                    Span::styled(ns.clone(), Style::new().fg(th.colors.fg_dim)),
                    Span::styled(pod.clone(), Style::new().fg(th.colors.fg)),
                    Span::styled(port.to_string(), Style::new().fg(th.colors.fg)),
                    Span::styled(age_secs(*age), Style::new().fg(th.colors.fg_dim)),
                ])
                .style(if i == self.selected {
                    Style::new().bg(th.colors.bg_active)
                } else {
                    Style::new()
                })
            });
        let table = Table::new(
            rows,
            [
                Constraint::Length(8),
                Constraint::Percentage(25),
                Constraint::Percentage(40),
                Constraint::Length(8),
                Constraint::Length(8),
            ],
        )
        .header(
            Row::new(["LOCAL", "NAMESPACE", "POD", "PORT", "AGE"])
                .style(Style::new().fg(th.colors.fg_dim).bold()),
        )
        .block(block);
        self.scroll.render(f, area, table, Some(self.selected));
    }
}
