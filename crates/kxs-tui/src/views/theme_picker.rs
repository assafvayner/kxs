//! Theme picker: `j/k` previews live, Enter persists, Esc reverts.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::cmd::Cmd;
use crate::theme::{all, Theme};
use crate::view::{Hint, View};
use crate::AppCtx;

pub struct ThemePicker {
    id: u64,
    themes: Vec<Theme>,
    selected: usize,
    /// Theme to restore on Esc.
    original: String,
}

impl ThemePicker {
    pub fn new(app: &mut crate::app::App) -> Self {
        ThemePicker {
            id: app.alloc_id(),
            themes: all(),
            selected: 0,
            original: app.theme.id.clone(),
        }
    }
}

impl View for ThemePicker {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        format!("Themes[{}]", self.themes.len())
    }

    fn crumb(&self) -> String {
        "theme".into()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![
            Hint::action("enter", "apply"),
            Hint::action("esc", "revert"),
        ]
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        let last = self.themes.len().saturating_sub(1);
        let mut cmds = vec![];
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Home | KeyCode::Char('g') => self.selected = 0,
            KeyCode::End | KeyCode::Char('G') => self.selected = last,
            KeyCode::Enter => {
                if let Some(t) = self.themes.get(self.selected) {
                    // re-anchor: a later Esc must revert to the applied
                    // theme, not the pre-picker one (disk already has it)
                    self.original = t.id.clone();
                    cmds.push(Cmd::PreviewTheme { id: t.id.clone() });
                    cmds.push(Cmd::SaveConfig);
                }
                return cmds;
            }
            KeyCode::Esc => {
                // revert the preview and pop
                cmds.push(Cmd::PreviewTheme {
                    id: self.original.clone(),
                });
                cmds.push(Cmd::PopView);
                return cmds;
            }
            _ => {}
        }
        if let Some(t) = self.themes.get(self.selected) {
            cmds.push(Cmd::PreviewTheme { id: t.id.clone() });
        }
        cmds
    }

    fn wants_filter(&self) -> bool {
        false
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, _filter: &str) {
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(th.colors.border))
            .title(Line::from(Span::styled(
                self.title(),
                Style::new().fg(th.colors.accent),
            )));
        let mut lines: Vec<Line> = Vec::new();
        for (i, t) in self.themes.iter().enumerate() {
            let selected = i == self.selected;
            let mut spans = vec![Span::styled(
                format!(" {}  ", t.label),
                if selected {
                    Style::new().bg(th.colors.bg_active).fg(th.colors.fg).bold()
                } else {
                    Style::new().fg(th.colors.fg)
                },
            )];
            // swatch strip: bg, accent, green, yellow, red
            for color in [
                t.colors.bg,
                t.colors.accent,
                t.colors.green,
                t.colors.yellow,
                t.colors.red,
            ] {
                spans.push(Span::styled("████", Style::new().fg(color)));
            }
            if t.id == self.original {
                spans.push(Span::styled(
                    "  (active)",
                    Style::new().fg(th.colors.fg_dim),
                ));
            }
            lines.push(Line::from(spans));
        }
        let rows = area.height.saturating_sub(2) as usize;
        let offset = self.selected.saturating_sub(rows.saturating_sub(1));
        f.render_widget(
            Paragraph::new(lines)
                .scroll((offset as u16, 0))
                .block(block),
            area,
        );
    }
}
