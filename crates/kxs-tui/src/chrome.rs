//! Header, footer, prompt, and flash — everything around the body view.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use tui_input::{Input, InputRequest};

use crate::cmd::Mutation;
use crate::theme::Theme;
use crate::view::{Hint, ViewId};
use kxs_cluster::discovery::ResourceKind;

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

/// A centered options picker; the choice is routed back to `for_view` as
/// `Msg::Picked`.
pub struct PickModal {
    pub title: String,
    pub options: Vec<(String, String)>,
    pub selected: usize,
    pub for_view: ViewId,
}

/// y/N confirmation for a mutation.
pub struct ConfirmModal {
    pub title: String,
    pub detail: String,
    pub for_view: ViewId,
    pub action: Mutation,
}

/// Text input for a mutation parameter (scale replicas).
pub struct InputModal {
    pub title: String,
    pub value: String,
    pub for_view: ViewId,
    pub action: InputAction,
}

#[derive(Clone)]
pub enum InputAction {
    Scale {
        kind: ResourceKind,
        ns: String,
        name: String,
    },
}

/// kubectl-style delete dialog: propagation policy + force toggle.
pub struct DeleteModal {
    pub title: String,
    pub for_view: ViewId,
    pub kind: ResourceKind,
    pub ns: String,
    pub name: String,
    pub propagation_idx: usize,
    pub force: bool,
}

pub const PROPAGATIONS: [&str; 3] = ["Background", "Foreground", "Orphan"];

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
    /// "12% / 43%" from the metrics poll; hidden until the first result.
    pub cpu_mem: Option<String>,
    pub pick: Option<PickModal>,
    pub confirm: Option<ConfirmModal>,
    pub input: Option<InputModal>,
    pub delete: Option<DeleteModal>,
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

    pub fn open_pick(&mut self, title: String, options: Vec<(String, String)>, for_view: ViewId) {
        self.pick = Some(PickModal {
            title,
            options,
            selected: 0,
            for_view,
        });
    }

    pub fn close_pick(&mut self) {
        self.pick = None;
    }

    /// Feed a key to the open pick modal.
    pub fn pick_key(&mut self, key: KeyEvent) -> PickOutcome {
        let Some(pick) = &mut self.pick else {
            return PickOutcome::Ignored;
        };
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                pick.selected = (pick.selected + 1).min(pick.options.len().saturating_sub(1));
                PickOutcome::Edited
            }
            KeyCode::Up | KeyCode::Char('k') => {
                pick.selected = pick.selected.saturating_sub(1);
                PickOutcome::Edited
            }
            KeyCode::Enter => {
                PickOutcome::Chose(pick.options.get(pick.selected).map(|(l, _)| l.clone()))
            }
            KeyCode::Esc => PickOutcome::Cancel,
            _ => PickOutcome::Ignored,
        }
    }

    pub fn open_confirm(
        &mut self,
        title: String,
        detail: String,
        for_view: ViewId,
        action: Mutation,
    ) {
        self.confirm = Some(ConfirmModal {
            title,
            detail,
            for_view,
            action,
        });
    }

    pub fn open_input(
        &mut self,
        title: String,
        prefill: String,
        for_view: ViewId,
        action: InputAction,
    ) {
        self.input = Some(InputModal {
            title,
            value: prefill,
            for_view,
            action,
        });
    }

    pub fn open_delete(
        &mut self,
        title: String,
        for_view: ViewId,
        kind: ResourceKind,
        ns: String,
        name: String,
    ) {
        self.delete = Some(DeleteModal {
            title,
            for_view,
            kind,
            ns,
            name,
            propagation_idx: 0,
            force: false,
        });
    }

    pub fn close_all_modals(&mut self) {
        self.pick = None;
        self.confirm = None;
        self.input = None;
        self.delete = None;
    }

    pub fn confirm_key(&mut self, key: KeyEvent) -> ConfirmOutcome {
        let Some(confirm) = &self.confirm else {
            return ConfirmOutcome::Ignored;
        };
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                ConfirmOutcome::Confirmed(Box::new(confirm.action.clone()))
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => ConfirmOutcome::Cancelled,
            _ => ConfirmOutcome::Ignored,
        }
    }

    pub fn input_key(&mut self, key: KeyEvent) -> InputOutcome {
        let Some(input) = &mut self.input else {
            return InputOutcome::Ignored;
        };
        match key.code {
            KeyCode::Enter => InputOutcome::Submitted(input.value.clone()),
            KeyCode::Esc => InputOutcome::Cancelled,
            KeyCode::Char(c) if c.is_ascii_digit() => {
                input.value.push(c);
                InputOutcome::Edited
            }
            KeyCode::Backspace => {
                input.value.pop();
                InputOutcome::Edited
            }
            _ => InputOutcome::Ignored,
        }
    }

    pub fn delete_key(&mut self, key: KeyEvent) -> DeleteOutcome {
        let Some(dm) = &mut self.delete else {
            return DeleteOutcome::Ignored;
        };
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                dm.propagation_idx = dm.propagation_idx.saturating_sub(1);
                DeleteOutcome::Edited
            }
            KeyCode::Right | KeyCode::Char('l') => {
                dm.propagation_idx = (dm.propagation_idx + 1).min(PROPAGATIONS.len() - 1);
                DeleteOutcome::Edited
            }
            KeyCode::Char('f') => {
                dm.force = !dm.force;
                DeleteOutcome::Edited
            }
            KeyCode::Enter => DeleteOutcome::Confirmed,
            KeyCode::Esc => DeleteOutcome::Cancelled,
            _ => DeleteOutcome::Ignored,
        }
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
            info.push(Line::from(vec![
                Span::styled("CPU/MEM: ", label),
                Span::styled(self.cpu_mem.clone().unwrap_or_else(|| "—".into()), value),
            ]));
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

    /// Centered modal for the pick dialog; takes precedence visually.
    pub fn render_pick(&self, f: &mut Frame, area: Rect, th: &Theme) {
        use ratatui::widgets::{Clear, List, ListItem};
        let Some(pick) = &self.pick else { return };
        let width = (area.width / 2).clamp(30, 60);
        let height = (pick.options.len() as u16 + 2).clamp(4, (area.height / 2).max(4));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let modal = Rect {
            x,
            y,
            width,
            height,
        };
        f.render_widget(Clear, modal);
        let items: Vec<ListItem> = pick
            .options
            .iter()
            .enumerate()
            .map(|(i, (label, hint))| {
                let selected = i == pick.selected;
                let mut spans = vec![Span::styled(
                    format!(" {label}"),
                    if selected {
                        Style::new().fg(th.colors.bg).bg(th.colors.accent)
                    } else {
                        Style::new().fg(th.colors.fg)
                    },
                )];
                if !hint.is_empty() {
                    spans.push(Span::styled(
                        format!("  {hint}"),
                        if selected {
                            Style::new().fg(th.colors.bg).bg(th.colors.accent)
                        } else {
                            Style::new().fg(th.colors.fg_dim)
                        },
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let list = List::new(items).block(
            ratatui::widgets::Block::new()
                .borders(ratatui::widgets::Borders::ALL)
                .border_style(Style::new().fg(th.colors.accent))
                .title(Line::from(Span::styled(
                    pick.title.clone(),
                    Style::new().fg(th.colors.accent),
                ))),
        );
        f.render_widget(list, modal);
    }

    fn modal_frame(&self, area: Rect, width: u16, height: u16) -> Rect {
        let width = width.clamp(30, area.width);
        let height = height.min(area.height);
        Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        }
    }

    pub fn render_confirm(&self, f: &mut Frame, area: Rect, th: &Theme) {
        use ratatui::widgets::{Clear, Paragraph};
        let Some(c) = &self.confirm else { return };
        let modal = self.modal_frame(area, 50, 5);
        f.render_widget(Clear, modal);
        let text = vec![
            Line::from(Span::styled(
                c.title.clone(),
                Style::new().fg(th.colors.accent).bold(),
            )),
            Line::from(Span::styled(
                c.detail.clone(),
                Style::new().fg(th.colors.fg),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "y confirm · n/Esc cancel",
                Style::new().fg(th.colors.fg_dim),
            )),
        ];
        f.render_widget(
            Paragraph::new(text).block(
                ratatui::widgets::Block::new()
                    .borders(ratatui::widgets::Borders::ALL)
                    .border_style(Style::new().fg(th.colors.red)),
            ),
            modal,
        );
    }

    pub fn render_input(&self, f: &mut Frame, area: Rect, th: &Theme) {
        use ratatui::widgets::{Clear, Paragraph};
        let Some(m) = &self.input else { return };
        let modal = self.modal_frame(area, 44, 5);
        f.render_widget(Clear, modal);
        let text = vec![
            Line::from(Span::styled(
                m.title.clone(),
                Style::new().fg(th.colors.accent).bold(),
            )),
            Line::from(Span::styled(
                format!("{}█", m.value),
                Style::new().fg(th.colors.fg),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Enter confirm · Esc cancel",
                Style::new().fg(th.colors.fg_dim),
            )),
        ];
        f.render_widget(
            Paragraph::new(text).block(
                ratatui::widgets::Block::new()
                    .borders(ratatui::widgets::Borders::ALL)
                    .border_style(Style::new().fg(th.colors.accent)),
            ),
            modal,
        );
    }

    pub fn render_delete(&self, f: &mut Frame, area: Rect, th: &Theme) {
        use ratatui::widgets::{Clear, Paragraph};
        let Some(dm) = &self.delete else { return };
        let modal = self.modal_frame(area, 56, 7);
        f.render_widget(Clear, modal);
        let props: Vec<String> = PROPAGATIONS
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if i == dm.propagation_idx {
                    format!("[{p}]")
                } else {
                    p.to_string()
                }
            })
            .collect();
        let text = vec![
            Line::from(Span::styled(
                dm.title.clone(),
                Style::new().fg(th.colors.red).bold(),
            )),
            Line::from(Span::styled(
                format!("propagation: {}", props.join("  ")),
                Style::new().fg(th.colors.fg),
            )),
            Line::from(Span::styled(
                format!("force: {}", if dm.force { "[x]" } else { "[ ]" }),
                Style::new().fg(th.colors.fg),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "←/→ propagation · f force · Enter delete · Esc cancel",
                Style::new().fg(th.colors.fg_dim),
            )),
        ];
        f.render_widget(
            Paragraph::new(text).block(
                ratatui::widgets::Block::new()
                    .borders(ratatui::widgets::Borders::ALL)
                    .border_style(Style::new().fg(th.colors.red)),
            ),
            modal,
        );
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
pub enum PickOutcome {
    /// Enter: the chosen option label (`None` when the list is empty).
    Chose(Option<String>),
    /// Esc: cancel.
    Cancel,
    Edited,
    Ignored,
}

pub enum ConfirmOutcome {
    Confirmed(Box<Mutation>),
    Cancelled,
    Ignored,
}

pub enum InputOutcome {
    Submitted(String),
    Cancelled,
    Edited,
    Ignored,
}

pub enum DeleteOutcome {
    Confirmed,
    Cancelled,
    Edited,
    Ignored,
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
