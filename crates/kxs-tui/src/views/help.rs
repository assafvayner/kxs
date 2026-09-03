//! Static help: global keys, view hints, and `:` commands.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::cmd::Cmd;
use crate::theme::Theme;
use crate::view::{Hint, View};
use crate::AppCtx;

const GLOBAL_KEYS: [(&str, &str); 14] = [
    (":", "command prompt"),
    ("/", "filter"),
    ("esc", "clear filter / back"),
    ("?", "help"),
    ("ctrl-c, :q", "quit"),
    ("0-9", "namespace favorites (0 = all)"),
    ("j/k, ↑/↓", "move selection"),
    ("g/G, Home/End", "first/last"),
    ("ctrl-f/b, PgUp/PgDn", "page"),
    ("shift-<col initial>", "sort by column"),
    ("c", "copy selected name"),
    ("ctrl-r", "refresh"),
    ("d", "describe"),
    ("y", "yaml"),
];

const COMMANDS: [(&str, &str); 6] = [
    (":<kind|alias> [ns]", "browse that kind"),
    (":ctx [name]", "switch context"),
    (":ns [name]", "switch namespace"),
    (":theme [id]", "set theme"),
    (":help", "this view"),
    (":q, :quit", "quit"),
];

pub struct HelpView {
    id: u64,
    scroll: u16,
}

impl HelpView {
    pub fn new(app: &mut crate::app::App) -> Self {
        HelpView {
            id: app.alloc_id(),
            scroll: 0,
        }
    }
}

impl View for HelpView {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        "Help".into()
    }

    fn crumb(&self) -> String {
        "help".into()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![Hint {
            key: "esc",
            desc: "back",
        }]
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Home | KeyCode::Char('g') => self.scroll = 0,
            _ => {}
        }
        vec![]
    }

    fn wants_filter(&self) -> bool {
        false
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, _filter: &str) {
        let key_style = Style::new().fg(th.colors.yellow).bold();
        let desc_style = Style::new().fg(th.colors.fg);
        let head_style = Style::new().fg(th.colors.accent).bold();
        let mut lines: Vec<Line> = vec![Line::from(Span::styled("GLOBAL KEYS", head_style))];
        for (k, d) in GLOBAL_KEYS {
            lines.push(Line::from(vec![
                Span::styled(format!("  {k:<24}"), key_style),
                Span::styled(d, desc_style),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("COMMANDS", head_style)));
        for (c, d) in COMMANDS {
            lines.push(Line::from(vec![
                Span::styled(format!("  {c:<24}"), key_style),
                Span::styled(d, desc_style),
            ]));
        }
        f.render_widget(
            Paragraph::new(lines)
                .scroll((self.scroll, 0))
                .wrap(Wrap { trim: false })
                .block(
                    Block::new()
                        .borders(Borders::ALL)
                        .border_style(Style::new().fg(th.colors.border))
                        .title(Line::from(Span::styled(
                            self.title(),
                            Style::new().fg(th.colors.accent),
                        ))),
                ),
            area,
        );
    }
}
