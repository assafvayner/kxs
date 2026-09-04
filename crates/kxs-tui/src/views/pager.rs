//! Shared text pager for Yaml and Describe views: scrolling, search, copy.

use std::cell::Cell;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::clipboard;
use crate::cmd::Cmd;
use crate::theme::Theme;
use crate::view::{Hint, View};
use crate::AppCtx;

pub struct Pager {
    id: u64,
    kind_label: String,
    lines: Vec<String>,
    /// Optional per-line colorizer (YAML syntax highlighting).
    colorize: Option<fn(&str, &Theme) -> Vec<Span<'static>>>,
    scroll: u16,
    viewport: Cell<u16>,
    search: Option<String>,
    match_lines: Vec<usize>,
    fullscreen: bool,
}

impl Pager {
    /// `f`: hand the whole frame to the text.
    pub fn toggle_fullscreen(&mut self) -> bool {
        self.fullscreen = !self.fullscreen;
        self.fullscreen
    }

    pub fn new(app: &mut crate::app::App, kind_label: String, text: &str) -> Self {
        Pager {
            id: app.alloc_id(),
            kind_label,
            lines: text.lines().map(String::from).collect(),
            colorize: None,
            scroll: 0,
            viewport: Cell::new(20),
            search: None,
            match_lines: vec![],
            fullscreen: false,
        }
    }

    pub fn with_colorizer(mut self, f: fn(&str, &Theme) -> Vec<Span<'static>>) -> Self {
        self.colorize = Some(f);
        self
    }

    pub fn set_text(&mut self, text: String) {
        self.lines = text.lines().map(String::from).collect();
    }

    fn max_scroll(&self) -> u16 {
        let visible = self.viewport.get().max(1);
        (self.lines.len() as u16).saturating_sub(visible)
    }

    fn find_matches(&mut self) {
        self.match_lines = match &self.search {
            Some(q) if !q.is_empty() => self
                .lines
                .iter()
                .enumerate()
                .filter(|(_, l)| l.to_lowercase().contains(&q.to_lowercase()))
                .map(|(i, _)| i)
                .collect(),
            _ => vec![],
        };
    }

    fn jump_match(&mut self, forward: bool) {
        if self.match_lines.is_empty() {
            return;
        }
        let cur = self.scroll as usize;
        let next = if forward {
            self.match_lines
                .iter()
                .find(|&&i| i > cur)
                .copied()
                .or_else(|| self.match_lines.first().copied())
        } else {
            self.match_lines
                .iter()
                .rev()
                .find(|&&i| i < cur)
                .copied()
                .or_else(|| self.match_lines.last().copied())
        };
        if let Some(i) = next {
            self.scroll = (i as u16).min(self.max_scroll());
        }
    }

    /// Line with search hits highlighted; falls back to the colorizer.
    fn highlight(&self, line: &str, th: &Theme) -> Vec<Span<'static>> {
        let Some(q) = self.search.as_ref().filter(|q| !q.is_empty()) else {
            return match self.colorize {
                Some(f) => f(line, th),
                None => vec![Span::styled(
                    line.to_string(),
                    Style::new().fg(th.colors.fg),
                )],
            };
        };
        highlight_spans(line, q, th)
    }
}

/// Case-insensitive highlight of `q` inside `line`, char-boundary safe.
pub(crate) fn highlight_spans(line: &str, q: &str, th: &Theme) -> Vec<Span<'static>> {
    let plain = |s: &str| Span::styled(s.to_string(), Style::new().fg(th.colors.fg));
    let hit_style = Style::new().fg(th.colors.bg).bg(th.colors.yellow);
    let ql = q.to_lowercase();
    let qchars = q.chars().count();
    let mut spans = vec![];
    let mut rest = line;
    while !rest.is_empty() {
        let found = rest
            .char_indices()
            .find(|(i, _)| rest[*i..].to_lowercase().starts_with(&ql));
        match found {
            Some((i, _)) => {
                if i > 0 {
                    spans.push(plain(&rest[..i]));
                }
                let hit_len: usize = rest[i..].chars().take(qchars).map(char::len_utf8).sum();
                spans.push(Span::styled(rest[i..i + hit_len].to_string(), hit_style));
                rest = &rest[i + hit_len..];
            }
            None => {
                spans.push(plain(rest));
                break;
            }
        }
    }
    spans
}

impl View for Pager {
    fn id(&self) -> u64 {
        self.id
    }

    fn title(&self) -> String {
        let mut t = self.kind_label.clone();
        if let Some(q) = &self.search {
            t.push_str(&format!("  /{q} ({} matches)", self.match_lines.len()));
        }
        t
    }

    fn crumb(&self) -> String {
        self.kind_label
            .split('(')
            .next()
            .unwrap_or("text")
            .to_lowercase()
    }

    fn hints(&self) -> Vec<Hint> {
        vec![
            Hint::action("/", "search"),
            Hint::action("n/N", "next/prev match"),
            Hint::action("c", "copy"),
            Hint::action("f", "fullscreen"),
        ]
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll = (self.scroll + 1).min(self.max_scroll())
            }
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageDown => {
                self.scroll = (self.scroll + self.viewport.get()).min(self.max_scroll())
            }
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(self.viewport.get()),
            KeyCode::Home | KeyCode::Char('g') => self.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => self.scroll = self.max_scroll(),
            KeyCode::Char('n') => self.jump_match(true),
            KeyCode::Char('N') => self.jump_match(false),
            KeyCode::Char('c') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                clipboard::copy(&self.lines.join("\n"));
            }
            _ => {}
        }
        vec![]
    }

    fn wants_filter(&self) -> bool {
        true
    }

    /// `/` search: highlights matches in the title and text.
    fn set_filter(&mut self, filter: &str) -> Vec<Cmd> {
        self.search = if filter.is_empty() {
            None
        } else {
            Some(filter.to_string())
        };
        self.find_matches();
        if let Some(first) = self.match_lines.first() {
            self.scroll = (*first as u16).min(self.max_scroll());
        }
        vec![]
    }

    fn filter(&self) -> String {
        self.search.clone().unwrap_or_default()
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, _filter: &str) {
        let inner_h = area.height.saturating_sub(2);
        self.viewport.set(inner_h);
        let start = self.scroll as usize;
        let end = (start + inner_h as usize).min(self.lines.len());
        let lines: Vec<Line> = self.lines[start..end]
            .iter()
            .map(|l| Line::from(self.highlight(l, th)))
            .collect();
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(th.colors.border))
            .title(Line::from(Span::styled(
                self.title(),
                Style::new().fg(th.colors.accent),
            )));
        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_handles_multibyte_text() {
        let spans = highlight_spans(
            "über Über ub",
            "ü",
            &crate::theme::get(crate::theme::DEFAULT_ID),
        );
        let hits: Vec<&str> = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(hits, vec!["ü", "ber ", "Ü", "ber ub"]);
    }
}
