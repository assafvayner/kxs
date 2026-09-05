//! Containers view for one pod (spec+status), init containers last and marked.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use kxs_cluster::pods::ContainerInfo;

use crate::cmd::{Cmd, Fetch};
use crate::theme::Theme;
use crate::view::{Hint, Target, View};
use crate::AppCtx;

pub struct ContainersView {
    id: u64,
    target: Target,
    containers: Vec<ContainerInfo>,
    selected: usize,
    selected_container: Option<String>,
    pending: bool,
    in_flight: bool,
    error: Option<String>,
    scroll: crate::table::Scroll,
}

impl ContainersView {
    pub fn new(app: &mut crate::app::App, target: Target) -> Self {
        ContainersView {
            id: app.alloc_id(),
            target,
            containers: vec![],
            selected: 0,
            selected_container: None,
            pending: true,
            in_flight: false,
            error: None,
            scroll: Default::default(),
        }
    }
}

impl View for ContainersView {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        format!(
            "Containers({}/{})[{}]",
            self.target.name,
            self.target.ns.as_deref().unwrap_or("all"),
            self.containers.len()
        )
    }

    fn crumb(&self) -> String {
        "containers".into()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![Hint::action("l", "logs"), Hint::action("s", "shell*")]
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        let last = self.containers.len().saturating_sub(1);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Home | KeyCode::Char('g') => self.selected = 0,
            KeyCode::End | KeyCode::Char('G') => self.selected = last,
            KeyCode::Char('l') => {
                // the app intercepts `l` and reads target().container
                self.selected_container =
                    self.containers.get(self.selected).map(|c| c.name.clone());
            }
            _ => {}
        }
        vec![]
    }

    fn on_msg(&mut self, msg: &crate::msg::Msg, _ctx: &AppCtx) -> Vec<Cmd> {
        match msg {
            crate::msg::Msg::Tick if self.pending && !self.in_flight && self.error.is_none() => {
                self.in_flight = true;
                vec![Cmd::Fetch {
                    view: self.id,
                    what: Fetch::Containers {
                        ns: self.target.ns.clone().unwrap_or_default(),
                        pod: self.target.name.clone(),
                    },
                }]
            }
            crate::msg::Msg::Fetched { result, .. } => {
                self.pending = false;
                self.in_flight = false;
                match result {
                    Ok(crate::cmd::FetchResult::Containers(infos)) => {
                        // regular containers first, init containers last: the same order render uses
                        let mut ordered: Vec<ContainerInfo> = infos
                            .iter()
                            .filter(|c| !c.init_container)
                            .cloned()
                            .collect();
                        ordered.extend(infos.iter().filter(|c| c.init_container).cloned());
                        self.containers = ordered;
                        self.selected = self.selected.min(self.containers.len().saturating_sub(1));
                    }
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

    fn target(&self) -> Option<Target> {
        let mut t = self.target.clone();
        if let Some(c) = self.containers.get(self.selected) {
            t.container = Some(c.name.clone());
        }
        Some(t)
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, _filter: &str) {
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(th.colors.border))
            .title(Line::from(Span::styled(
                self.title(),
                Style::new().fg(th.colors.accent),
            )));
        if self.containers.is_empty() {
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
        let rows = self.containers.iter().enumerate().map(|(i, c)| {
            let init = if c.init_container { "init" } else { "" };
            let state = if c.state.is_empty() {
                if c.ready {
                    "running"
                } else {
                    "waiting"
                }
            } else {
                c.state.as_str()
            };
            let ports = c
                .ports
                .iter()
                .map(|p| p.container_port.to_string())
                .collect::<Vec<_>>()
                .join(",");
            Row::new(vec![
                Span::styled(c.name.clone(), Style::new().fg(th.colors.fg)),
                Span::styled(c.image.clone(), Style::new().fg(th.colors.fg_dim)),
                Span::styled(
                    if c.ready { "✓" } else { "✗" },
                    Style::new().fg(if c.ready {
                        th.colors.green
                    } else {
                        th.colors.yellow
                    }),
                ),
                Span::styled(state.to_string(), Style::new().fg(th.colors.fg_dim)),
                Span::styled(c.restarts.to_string(), Style::new().fg(th.colors.fg)),
                Span::styled(ports, Style::new().fg(th.colors.fg_dim)),
                Span::styled(init.to_string(), Style::new().fg(th.colors.fg_dim)),
            ])
            .style(if i == self.selected {
                Style::new().bg(th.colors.bg_active)
            } else {
                Style::new()
            })
        });
        let widths = [
            Constraint::Percentage(22),
            Constraint::Percentage(34),
            Constraint::Length(5),
            Constraint::Length(9),
            Constraint::Length(10),
            Constraint::Percentage(12),
            Constraint::Length(5),
        ];
        let table = Table::new(rows, widths)
            .header(
                Row::new([
                    "NAME", "IMAGE", "READY", "STATE", "RESTARTS", "PORTS", "INIT",
                ])
                .style(Style::new().fg(th.colors.fg_dim).bold()),
            )
            .block(block);
        self.scroll.render(f, area, table, Some(self.selected));
    }
}
