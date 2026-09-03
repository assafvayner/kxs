//! Header, footer, prompt, and flash — everything around the body view.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use tui_input::{Input, InputRequest};

use crate::theme::Theme;
use crate::view::Hint;

pub const FLASH_DURATION: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Command,
    Filter,
}

impl PromptKind {
    pub fn prefix(self) -> &'static str {
        match self {
            PromptKind::Command => ":",
            PromptKind::Filter => "/",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Prompt {
    pub kind: PromptKind,
    pub input: Input,
}

#[derive(Debug, Clone)]
pub struct Flash {
    pub text: String,
    pub error: bool,
    pub until: Instant,
}

/// Chrome owns everything that is not a body view: header data, the `:`/`/`
/// prompt, flash messages, and (later phases) modals.
#[derive(Default)]
pub struct Chrome {
    pub context: String,
    pub cluster: String,
    pub user: String,
    pub version: String,
    /// Namespace of the active context; `None` = all.
    pub namespace: Option<String>,
    /// Namespace favorites for the header column; index 0 is the `0` key.
    pub favorites: Vec<String>,
    pub prompt: Option<Prompt>,
    pub flash: Option<Flash>,
    pub size: (u16, u16),
}

impl Chrome {
    pub fn flash(&mut self, text: impl Into<String>, error: bool) {
        self.flash = Some(Flash {
            text: text.into(),
            error,
            until: Instant::now() + FLASH_DURATION,
        });
    }

    pub fn tick(&mut self) {
        if let Some(f) = &self.flash {
            if Instant::now() >= f.until {
                self.flash = None;
            }
        }
    }

    pub fn open_prompt(&mut self, kind: PromptKind) {
        self.prompt = Some(Prompt {
            kind,
            input: Input::default(),
        });
    }

    pub fn close_prompt(&mut self) {
        self.prompt = None;
    }

    /// Feed a key to the active prompt. Returns `Some(submitted_text)` on
    /// Enter, `None` otherwise. `Esc` closes the prompt (returns empty
    /// submit? no — `None`, caller closes via `close_prompt`).
    pub fn prompt_key(&mut self, key: KeyEvent) -> PromptOutcome {
        let Some(prompt) = &mut self.prompt else {
            return PromptOutcome::Ignored;
        };
        let req = match key.code {
            KeyCode::Enter => return PromptOutcome::Submit(prompt.input.value().to_string()),
            KeyCode::Esc => return PromptOutcome::Cancel,
            KeyCode::Char(c) => InputRequest::InsertChar(c),
            KeyCode::Backspace => InputRequest::DeletePrevChar,
            KeyCode::Delete => InputRequest::DeleteNextChar,
            KeyCode::Left => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    InputRequest::GoToPrevWord
                } else {
                    InputRequest::GoToPrevChar
                }
            }
            KeyCode::Right => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    InputRequest::GoToNextWord
                } else {
                    InputRequest::GoToNextChar
                }
            }
            KeyCode::Home => InputRequest::GoToStart,
            KeyCode::End => InputRequest::GoToEnd,
            _ => return PromptOutcome::Ignored,
        };
        prompt.input.handle(req);
        PromptOutcome::Edited
    }

    /// Header height at the given width: 2 rows under 80 columns (context +
    /// hints only), else 5.
    pub fn header_height(width: u16) -> u16 {
        if width < 80 {
            2
        } else {
            5
        }
    }

    pub fn render_header(&self, f: &mut Frame, area: Rect, th: &Theme, hints: &[Hint]) {
        let label = Style::new().fg(th.colors.fg_dim);
        let value = Style::new().fg(th.colors.fg);
        let accent = Style::new().fg(th.colors.accent);
        if area.width < 80 {
            let ctx_line = Line::from(vec![
                Span::styled("Context: ", label),
                Span::styled(self.context.clone(), accent),
                Span::styled(
                    format!("  ns: {}", self.namespace.as_deref().unwrap_or("all")),
                    label,
                ),
            ]);
            f.render_widget(Paragraph::new(ctx_line), area);
            let h = hint_spans(hints, th, area.width);
            if area.height > 1 {
                f.render_widget(
                    Paragraph::new(h),
                    Rect {
                        y: area.y + 1,
                        height: 1,
                        ..area
                    },
                );
            }
            return;
        }
        let mut info = vec![
            Line::from(vec![
                Span::styled("Context: ", label),
                Span::styled(self.context.clone(), accent),
            ]),
            Line::from(vec![
                Span::styled("Cluster: ", label),
                Span::styled(
                    truncate(&self.cluster, (area.width / 3).max(10) as usize),
                    value,
                ),
            ]),
            Line::from(vec![
                Span::styled("User:    ", label),
                Span::styled(
                    truncate(&self.user, (area.width / 3).max(10) as usize),
                    value,
                ),
            ]),
            Line::from(vec![
                Span::styled("K8s Rev: ", label),
                Span::styled(self.version.clone(), value),
            ]),
        ];
        if area.height > 4 {
            info.push(Line::from(vec![Span::styled("CPU/MEM: —", label)]));
        }
        let info_w = (area.width / 3).clamp(16, 44);
        f.render_widget(
            Paragraph::new(info),
            Rect {
                x: area.x,
                y: area.y,
                width: info_w,
                height: area.height,
            },
        );

        // namespace favorites column
        if area.width > info_w {
            let mut ns_lines = Vec::new();
            ns_lines.push(Line::from(vec![
                Span::styled("<0> ", label),
                Span::styled(
                    "all",
                    if self.namespace.is_none() {
                        accent
                    } else {
                        value
                    },
                ),
            ]));
            for (i, fav) in self.favorites.iter().take(4).enumerate() {
                let active = self.namespace.as_deref() == Some(fav.as_str());
                ns_lines.push(Line::from(vec![
                    Span::styled(format!("<{}> ", i + 1), label),
                    Span::styled(truncate(fav, 24), if active { accent } else { value }),
                ]));
            }
            f.render_widget(
                Paragraph::new(ns_lines),
                Rect {
                    x: area.x + info_w + 1,
                    y: area.y,
                    width: area.width - info_w - 1,
                    height: area.height,
                },
            );
        }

        // hotkey hints, as many columns as fit, right-aligned area before logo
        let logo_w = if area.width >= 100 { 18 } else { 0 };
        let hints_end = area.width.saturating_sub(logo_w);
        let hints_start = (info_w * 2 + 2).min(hints_end);
        if hints_end > hints_start {
            let hint_area = Rect {
                x: area.x + hints_start,
                y: area.y,
                width: hints_end - hints_start,
                height: area.height,
            };
            let columns = hint_columns(hints, th, hint_area.width);
            let mut lines: Vec<Line> = Vec::new();
            let per_col = 2;
            for chunk in columns.chunks(per_col) {
                lines.push(Line::from(chunk.to_vec()));
            }
            f.render_widget(Paragraph::new(lines), hint_area);
        }

        // logo, right side, only when wide
        if logo_w > 0 && area.height >= 4 {
            let logo = [
                "  _  _____ ___ ",
                " | |/ / _ \\ __|",
                " | ' <  __/ _| ",
                " |_|\\_\\___|___|",
            ];
            let logo_style = Style::new()
                .fg(th.colors.accent)
                .add_modifier(Modifier::BOLD);
            let x = area.x + area.width - logo_w;
            for (i, row) in logo.iter().enumerate() {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(*row, logo_style))),
                    Rect {
                        x,
                        y: area.y + i as u16,
                        width: logo_w,
                        height: 1,
                    },
                );
            }
        }
    }

    pub fn render_footer(&self, f: &mut Frame, area: Rect, th: &Theme, crumbs: &[String]) {
        let label = Style::new().fg(th.colors.fg_dim);
        let mut spans: Vec<Span> = Vec::new();
        for (i, c) in crumbs.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" > ", label));
            }
            spans.push(Span::styled(
                format!("<{c}>"),
                Style::new().fg(th.colors.accent),
            ));
        }
        if let Some(flash) = &self.flash {
            let style = if flash.error {
                Style::new().fg(th.colors.red)
            } else {
                Style::new().fg(th.colors.green)
            };
            let used: usize = spans.iter().map(|s| s.width()).sum();
            let avail = area.width as usize;
            let text = truncate(&flash.text, avail.saturating_sub(used + 1));
            if !text.is_empty() {
                spans.push(Span::styled(" ", label));
                spans.push(Span::styled(text, style));
            }
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    /// The prompt line that replaces the body title row while active.
    pub fn render_prompt(&self, f: &mut Frame, area: Rect, th: &Theme) {
        let Some(prompt) = &self.prompt else { return };
        let input = prompt.input.value();
        let cursor = prompt.input.visual_cursor();
        let width = area.width.saturating_sub(3) as usize;
        let scroll = prompt.input.visual_scroll(width);
        let shown: String = input.chars().skip(scroll).take(width).collect();
        let vis_cursor = cursor.saturating_sub(scroll);
        let mut spans = vec![
            Span::styled(prompt.kind.prefix(), Style::new().fg(th.colors.accent)),
            Span::styled(" ", Style::new()),
        ];
        if vis_cursor >= shown.chars().count() {
            spans.push(Span::styled(shown, Style::new().fg(th.colors.fg)));
            spans.push(Span::styled("█", Style::new().fg(th.colors.accent)));
        } else {
            let chars: Vec<char> = shown.chars().collect();
            let (before, at) = chars.split_at(vis_cursor);
            let before: String = before.iter().collect();
            let at: String = at.first().map(|c| c.to_string()).unwrap_or_default();
            let after: String = at
                .chars()
                .skip(1)
                .chain(chars[vis_cursor + 1..].iter().copied())
                .collect();
            if !before.is_empty() {
                spans.push(Span::styled(before, Style::new().fg(th.colors.fg)));
            }
            spans.push(Span::styled(
                at,
                Style::new().fg(th.colors.fg).bg(th.colors.bg_active),
            ));
            spans.push(Span::styled(after, Style::new().fg(th.colors.fg)));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

#[derive(Debug, PartialEq)]
pub enum PromptOutcome {
    /// Enter: submit the current text.
    Submit(String),
    /// Esc: caller closes the prompt.
    Cancel,
    Edited,
    Ignored,
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

fn hint_spans(hints: &[Hint], th: &Theme, width: u16) -> Line<'static> {
    let spans = hint_columns(hints, th, width);
    if spans.is_empty() {
        Line::default()
    } else {
        Line::from(spans)
    }
}

/// Hint pairs laid out left-to-right until the width is exhausted; returns
/// up to two rows' worth as flattened span lists per column pair.
fn hint_columns(hints: &[Hint], th: &Theme, width: u16) -> Vec<Span<'static>> {
    let key_style = Style::new()
        .fg(th.colors.yellow)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::new().fg(th.colors.fg_dim);
    let mut spans = Vec::new();
    let mut used = 0usize;
    for h in hints {
        let item_len = h.key.chars().count() + h.desc.chars().count() + 3;
        if used + item_len > width as usize {
            break;
        }
        used += item_len;
        spans.push(Span::styled(format!("<{}>", h.key), key_style));
        spans.push(Span::styled(format!("{}  ", h.desc), desc_style));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_height_collapse() {
        assert_eq!(Chrome::header_height(79), 2);
        assert_eq!(Chrome::header_height(80), 5);
        assert_eq!(Chrome::header_height(160), 5);
    }

    #[test]
    fn flash_expires_after_five_seconds() {
        let mut c = Chrome::default();
        c.flash("hi", false);
        c.tick();
        assert!(c.flash.is_some());
        c.flash = Some(Flash {
            text: "old".into(),
            error: false,
            until: Instant::now() - Duration::from_secs(1),
        });
        c.tick();
        assert!(c.flash.is_none());
    }

    #[test]
    fn prompt_outcomes() {
        let mut c = Chrome::default();
        assert_eq!(c.prompt_key(KeyCode::Enter.into()), PromptOutcome::Ignored);
        c.open_prompt(PromptKind::Command);
        // feed plain KeyEvents via a helper
        fn key(c: char) -> KeyEvent {
            KeyEvent::from(KeyCode::Char(c))
        }
        c.prompt_key(key('d'));
        c.prompt_key(key('e'));
        assert!(matches!(c.prompt_key(key('s')), PromptOutcome::Edited));
        match c.prompt_key(KeyEvent::from(KeyCode::Enter)) {
            PromptOutcome::Submit(text) => assert_eq!(text, "des"),
            other => panic!("expected submit, got {other:?}"),
        }
    }

    #[test]
    fn esc_cancels_prompt() {
        let mut c = Chrome::default();
        c.open_prompt(PromptKind::Filter);
        assert!(matches!(
            c.prompt_key(KeyEvent::from(KeyCode::Esc)),
            PromptOutcome::Cancel
        ));
        c.close_prompt();
        assert!(c.prompt.is_none());
    }
}
