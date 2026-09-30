//! Aliases view (`ctrl-a`, `:alias`): every discovered kind and the shortcuts
//! that resolve to it, so `:` targets can be found without guessing.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use ratatui::Frame;

use kxs_cluster::discovery::ResourceKind;

use crate::cmd::Cmd;
use crate::theme::Theme;
use crate::view::{Hint, View};
use crate::AppCtx;

pub struct AliasesView {
    id: u64,
    kinds: Vec<ResourceKind>,
    selected: usize,
    filter: String,
    scroll: crate::table::Scroll,
}

impl AliasesView {
    pub fn new(app: &mut crate::app::App) -> Self {
        let ctx = app.ctx();
        let mut kinds: Vec<ResourceKind> =
            kxs_cluster::command::visible_kinds(&ctx.kinds, ctx.present.as_deref())
                .into_iter()
                .cloned()
                .collect();
        kinds.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.group.cmp(&b.group)));
        AliasesView {
            id: app.alloc_id(),
            kinds,
            selected: 0,
            filter: String::new(),
            scroll: Default::default(),
        }
    }

    /// Kinds surviving the `/` filter; the filter also searches the aliases.
    fn visible(&self) -> Vec<&ResourceKind> {
        let pred = kxs_cluster::table::filter_predicate(&self.filter);
        self.kinds
            .iter()
            .filter(|k| pred(&format!("{} {} {}", k.kind, k.plural, k.aliases.join(" "))))
            .collect()
    }

    fn gvr(k: &ResourceKind) -> String {
        if k.group.is_empty() {
            format!("{}/{}", k.version, k.plural)
        } else {
            format!("{}/{}/{}", k.group, k.version, k.plural)
        }
    }
}

impl View for AliasesView {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        format!("Aliases[{}]", self.visible().len())
    }

    fn crumb(&self) -> String {
        "aliases".into()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![Hint::action("enter", "view"), Hint::action("/", "filter")]
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        let last = self.visible().len().saturating_sub(1);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::PageDown => self.selected = (self.selected + 10).min(last),
            KeyCode::PageUp => self.selected = self.selected.saturating_sub(10),
            KeyCode::Home | KeyCode::Char('g') => self.selected = 0,
            KeyCode::End | KeyCode::Char('G') => self.selected = last,
            KeyCode::Enter => {
                if let Some(k) = self.visible().get(self.selected) {
                    return vec![Cmd::OpenKind {
                        query: k.plural.clone(),
                    }];
                }
            }
            _ => {}
        }
        vec![]
    }

    fn wants_filter(&self) -> bool {
        true
    }

    fn filter(&self) -> String {
        self.filter.clone()
    }

    fn set_filter(&mut self, filter: &str) -> Vec<Cmd> {
        self.filter = filter.to_string();
        self.selected = self.selected.min(self.visible().len().saturating_sub(1));
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
        let visible = self.visible();
        if visible.is_empty() {
            f.render_widget(block, area);
            f.render_widget(
                Paragraph::new("no kinds discovered").style(Style::new().fg(th.colors.fg_dim)),
                Rect {
                    x: area.x + 2,
                    y: area.y + 1,
                    width: area.width.saturating_sub(4),
                    height: 1,
                },
            );
            return;
        }
        let rows = visible.iter().enumerate().map(|(i, k)| {
            Row::new(vec![
                Span::styled(k.kind.clone(), Style::new().fg(th.colors.fg)),
                Span::styled(k.aliases.join(", "), Style::new().fg(th.colors.yellow)),
                Span::styled(Self::gvr(k), Style::new().fg(th.colors.fg_dim)),
                Span::styled(
                    if k.namespaced {
                        "namespaced"
                    } else {
                        "cluster"
                    }
                    .to_string(),
                    Style::new().fg(th.colors.fg_dim),
                ),
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
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(35),
                Constraint::Length(11),
            ],
        )
        .header(
            Row::new(["KIND", "ALIASES", "GVR", "SCOPE"])
                .style(Style::new().fg(th.colors.fg_dim).bold()),
        )
        .block(block);
        self.scroll.render(f, area, table, Some(self.selected));
    }
}
