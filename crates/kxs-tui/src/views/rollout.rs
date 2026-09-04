//! Rollout history for a Deployment; Enter confirms undo to a revision.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use kxs_cluster::workloads::RolloutRevision;

use crate::cmd::{Cmd, Fetch, StopHandle};
use crate::theme::Theme;
use crate::view::{Hint, Target, View};
use crate::AppCtx;

pub struct RolloutView {
    id: u64,
    target: Target,
    revisions: Vec<RolloutRevision>,
    selected: usize,
    handle: Option<StopHandle>,
    pending: bool,
    in_flight: bool,
    error: Option<String>,
}

impl RolloutView {
    pub fn new(app: &mut crate::app::App, target: Target) -> Self {
        RolloutView {
            id: app.alloc_id(),
            target,
            revisions: vec![],
            selected: 0,
            handle: None,
            pending: true,
            in_flight: false,
            error: None,
        }
    }
}

impl View for RolloutView {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        format!("Rollout({})", self.target.name)
    }

    fn crumb(&self) -> String {
        "rollout".into()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![Hint::mutation("enter", "undo to revision")]
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        let last = self.revisions.len().saturating_sub(1);
        let mut cmds = vec![];
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Enter => {
                if let Some(rev) = self.revisions.get(self.selected) {
                    if !rev.current {
                        cmds.push(Cmd::ConfirmUndo {
                            view: self.id,
                            ns: self.target.ns.clone().unwrap_or_default(),
                            name: self.target.name.clone(),
                            revision: rev.revision,
                        });
                    }
                }
            }
            _ => {}
        }
        cmds
    }

    fn on_started(&mut self, handle: StopHandle, _ctx: &AppCtx) -> Vec<Cmd> {
        self.handle = Some(handle);
        vec![]
    }

    fn on_msg(&mut self, msg: &crate::msg::Msg, _ctx: &AppCtx) -> Vec<Cmd> {
        match msg {
            crate::msg::Msg::Tick if self.pending && !self.in_flight => {
                self.in_flight = true;
                vec![Cmd::Fetch {
                    view: self.id,
                    what: Fetch::RolloutHistory {
                        ns: self.target.ns.clone().unwrap_or_default(),
                        name: self.target.name.clone(),
                    },
                }]
            }
            crate::msg::Msg::Fetched {
                result: Ok(crate::cmd::FetchResult::Rollout(revs)),
                ..
            } => {
                self.pending = false;
                self.in_flight = false;
                self.revisions = revs.clone();
                vec![]
            }
            crate::msg::Msg::Fetched { result: Err(e), .. } => {
                self.pending = false;
                self.in_flight = false;
                self.revisions = vec![];
                self.error = Some(e.clone());
                vec![]
            }
            _ => vec![],
        }
    }

    fn wants_filter(&self) -> bool {
        false
    }

    fn target(&self) -> Option<Target> {
        Some(self.target.clone())
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, _filter: &str) {
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(th.colors.border))
            .title(Line::from(Span::styled(
                self.title(),
                Style::new().fg(th.colors.accent),
            )));
        if self.revisions.is_empty() {
            f.render_widget(block, area);
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
        let rows = self.revisions.iter().enumerate().map(|(i, r)| {
            let images = r.images.join(", ");
            Row::new(vec![
                Span::styled(r.revision.to_string(), Style::new().fg(th.colors.fg)),
                Span::styled(
                    r.created
                        .as_deref()
                        .map(|c| kxs_core::format::age(Some(c), kxs_cluster::clock::now_ms()))
                        .unwrap_or_else(|| "—".into()),
                    Style::new().fg(th.colors.fg_dim),
                ),
                Span::styled(images, Style::new().fg(th.colors.fg)),
                Span::styled(
                    if r.current {
                        "current".into()
                    } else {
                        String::new()
                    },
                    Style::new().fg(th.colors.green),
                ),
            ])
            .style(if i == self.selected {
                Style::new().bg(th.colors.bg_active)
            } else {
                Style::new()
            })
        });
        f.render_widget(
            Table::new(
                rows,
                [
                    Constraint::Length(10),
                    Constraint::Length(10),
                    Constraint::Percentage(60),
                    Constraint::Length(10),
                ],
            )
            .header(
                Row::new(["REVISION", "AGE", "IMAGES", "CURRENT"])
                    .style(Style::new().fg(th.colors.fg_dim).bold()),
            )
            .block(block),
            area,
        );
    }
}
