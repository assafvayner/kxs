//! Metrics view: node utilization bars on top, pods sorted by CPU below.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use kxs_cluster::metrics::{MetricsRow, NodeMetricsRow};
use kxs_cluster::utilization::{bar, cpu_util, mem_util, of_total, percent};

use crate::cmd::Cmd;
use crate::theme::Theme;
use crate::view::{Hint, View};
use crate::AppCtx;

pub struct MetricsView {
    id: u64,
    pods: Result<Vec<MetricsRow>, String>,
    nodes: Result<Vec<NodeMetricsRow>, String>,
    selected: usize,
    /// Poll interval, kept so `ctrl-r` can restart the poller with it.
    interval: std::time::Duration,
    nodes_scroll: crate::table::Scroll,
    pods_scroll: crate::table::Scroll,
}

impl MetricsView {
    pub fn new(app: &mut crate::app::App) -> Self {
        let secs = app
            .config
            .lock()
            .map(|c| c.metrics_interval_secs)
            .unwrap_or(15);
        MetricsView {
            id: app.alloc_id(),
            pods: Ok(vec![]),
            nodes: Ok(vec![]),
            selected: 0,
            interval: std::time::Duration::from_secs(secs),
            nodes_scroll: Default::default(),
            pods_scroll: Default::default(),
        }
    }

    fn color_for(cls: &str, th: &Theme) -> Style {
        match cls {
            "st-bad" => Style::new().fg(th.colors.red),
            "st-warn" => Style::new().fg(th.colors.yellow),
            _ => Style::new().fg(th.colors.fg),
        }
    }
}

impl View for MetricsView {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        let nodes = self.nodes.as_ref().map(|n| n.len()).unwrap_or(0);
        let pods = self.pods.as_ref().map(|p| p.len()).unwrap_or(0);
        format!("Metrics[nodes={nodes} pods={pods}]")
    }

    fn crumb(&self) -> String {
        "metrics".into()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![Hint::action("ctrl-r", "refresh")]
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.selected = self.selected.saturating_add(1),
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::PageDown => self.selected = self.selected.saturating_add(10),
            KeyCode::PageUp => self.selected = self.selected.saturating_sub(10),
            KeyCode::Home | KeyCode::Char('g') => self.selected = 0,
            // restarting the poller ticks immediately, so this is a refresh
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return vec![Cmd::PollMetrics {
                    view: self.id,
                    every: self.interval,
                }];
            }
            _ => {}
        }
        vec![]
    }

    fn wants_filter(&self) -> bool {
        false
    }

    fn on_msg(&mut self, msg: &crate::msg::Msg, _ctx: &AppCtx) -> Vec<Cmd> {
        if let crate::msg::Msg::Metrics { pods, nodes, .. } = msg {
            self.pods = pods.clone();
            self.nodes = nodes.clone();
        }
        vec![]
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, _filter: &str) {
        let [nodes_area, pods_area] =
            Layout::vertical([Constraint::Percentage(35), Constraint::Percentage(65)]).areas(area);
        let nodes_block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(th.colors.border))
            .title(Line::from(Span::styled(
                "Nodes",
                Style::new().fg(th.colors.accent),
            )));
        let pods_block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(th.colors.border))
            .title(Line::from(Span::styled(
                "Pods by CPU",
                Style::new().fg(th.colors.accent),
            )));

        match &self.nodes {
            Err(e) => {
                f.render_widget(nodes_block, nodes_area);
                f.render_widget(
                    Paragraph::new(format!("metrics-server: {e}"))
                        .style(Style::new().fg(th.colors.fg_dim)),
                    nodes_area,
                );
            }
            Ok(nodes) if nodes.is_empty() => {
                f.render_widget(nodes_block, nodes_area);
                f.render_widget(
                    Paragraph::new("no node metrics (metrics-server absent?)")
                        .style(Style::new().fg(th.colors.fg_dim)),
                    nodes_area,
                );
            }
            Ok(nodes) => {
                let rows = nodes.iter().map(|n| {
                    let cpu = of_total(n.cpu_millicores, n.cpu_allocatable_millicores, "m");
                    let mem = of_total(n.mem_mib, n.mem_allocatable_mib, "Mi");
                    let cpu_pct = percent(n.cpu_millicores, n.cpu_allocatable_millicores);
                    let mem_pct = percent(n.mem_mib, n.mem_allocatable_mib);
                    Row::new(vec![
                        Span::styled(n.name.clone(), Style::new().fg(th.colors.fg)),
                        Span::styled(bar(cpu_pct, 10), Style::new().fg(th.colors.accent)),
                        Span::styled(cpu.text.clone(), Self::color_for(cpu.cls, th)),
                        Span::styled(bar(mem_pct, 10), Style::new().fg(th.colors.accent)),
                        Span::styled(mem.text.clone(), Self::color_for(mem.cls, th)),
                    ])
                });
                let table = Table::new(
                    rows,
                    [
                        Constraint::Percentage(25),
                        Constraint::Length(12),
                        Constraint::Percentage(20),
                        Constraint::Length(12),
                        Constraint::Percentage(20),
                    ],
                )
                .header(
                    Row::new(["NAME", "CPU", "CPU%", "MEM", "MEM%"])
                        .style(Style::new().fg(th.colors.fg_dim).bold()),
                )
                .block(nodes_block);
                self.nodes_scroll.render(f, nodes_area, table, None);
            }
        }

        match &self.pods {
            Err(_) | Ok(_) => {
                let mut pods = self.pods.as_ref().unwrap_or(&vec![]).clone();
                pods.sort_by_key(|p| std::cmp::Reverse(p.cpu_millicores));
                let selected = self.selected.min(pods.len().saturating_sub(1));
                let rows = pods.iter().enumerate().map(|(i, p)| {
                    let cpu = cpu_util(Some(p.cpu_millicores), None);
                    let mem = mem_util(Some(p.mem_mib), None);
                    Row::new(vec![
                        Span::styled(
                            p.namespace.clone().unwrap_or_default(),
                            Style::new().fg(th.colors.fg_dim),
                        ),
                        Span::styled(p.name.clone(), Style::new().fg(th.colors.fg)),
                        Span::styled(cpu.text.clone(), Self::color_for(cpu.cls, th)),
                        Span::styled(mem.text.clone(), Self::color_for(mem.cls, th)),
                    ])
                    .style(if i == selected {
                        Style::new().bg(th.colors.bg_active)
                    } else {
                        Style::new()
                    })
                });
                let table = Table::new(
                    rows,
                    [
                        Constraint::Percentage(20),
                        Constraint::Percentage(40),
                        Constraint::Percentage(20),
                        Constraint::Percentage(20),
                    ],
                )
                .header(
                    Row::new(["NAMESPACE", "NAME", "CPU", "MEM"])
                        .style(Style::new().fg(th.colors.fg_dim).bold()),
                )
                .block(pods_block);
                let sel = (!pods.is_empty()).then_some(selected);
                self.pods_scroll.render(f, pods_area, table, sel);
            }
        }
    }
}
