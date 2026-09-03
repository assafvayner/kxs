//! YAML view with a small line-based syntax colorizer.

use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::Frame;

use crate::cmd::Cmd;
use crate::theme::Theme;
use crate::view::{Hint, Target, View};
use crate::views::pager::Pager;
use crate::AppCtx;

pub struct YamlView {
    pager: Pager,
}

/// Line-based YAML tokenizer: keys, scalars, comments, punctuation.
pub fn colorize(line: &str, th: &Theme) -> Vec<Span<'static>> {
    let key_style = Style::new().fg(th.colors.syn_key);
    let string_style = Style::new().fg(th.colors.syn_string);
    let number_style = Style::new().fg(th.colors.syn_number);
    let comment_style = Style::new().fg(th.colors.syn_comment);
    let punct_style = Style::new().fg(th.colors.syn_punct);
    let plain = Style::new().fg(th.colors.fg);

    let (code, comment) = match line.find(" #") {
        Some(i) => (&line[..i], Some(&line[i..])),
        None if line.starts_with('#') => ("", Some(line)),
        _ => (line, None),
    };

    let mut spans: Vec<Span> = Vec::new();
    let trimmed = code.trim_start();
    let indent = code.len() - trimmed.len();
    if indent > 0 {
        spans.push(Span::styled(code[..indent].to_string(), plain));
    }
    if let Some((key, rest)) = trimmed.split_once(':') {
        spans.push(Span::styled(key.to_string(), key_style));
        spans.push(Span::styled(":", punct_style));
        let value_indent = rest.len() - rest.trim_start().len();
        if value_indent > 0 {
            spans.push(Span::styled(rest[..value_indent].to_string(), plain));
        }
        let value = rest.trim();
        if value.is_empty() {
            // nothing after the colon
        } else if value.starts_with('"') || value.starts_with('\'') {
            spans.push(Span::styled(value.to_string(), string_style));
        } else if value.parse::<f64>().is_ok()
            || value == "true"
            || value == "false"
            || value == "null"
        {
            spans.push(Span::styled(value.to_string(), number_style));
        } else {
            spans.push(Span::styled(value.to_string(), string_style));
        }
    } else if let Some(item) = trimmed.strip_prefix("- ") {
        spans.push(Span::styled("-".to_string(), punct_style));
        spans.push(Span::styled(format!(" {item}"), plain));
    } else if !trimmed.is_empty() {
        spans.push(Span::styled(trimmed.to_string(), plain));
    }
    if let Some(c) = comment {
        spans.push(Span::styled(c.to_string(), comment_style));
    }
    if spans.is_empty() {
        spans.push(Span::styled(line.to_string(), plain));
    }
    spans
}

impl YamlView {
    pub fn new(app: &mut crate::app::App, target: &Target) -> Self {
        let label = format!("YAML({}/{})", target.kind.kind, target.name);
        let pager = Pager::new(app, label, "").with_colorizer(colorize);
        YamlView { pager }
    }

    pub fn set_text(&mut self, text: String) {
        self.pager.set_text(text);
    }
}

impl View for YamlView {
    fn id(&self) -> u64 {
        self.pager.id()
    }

    fn title(&self) -> String {
        self.pager.title()
    }

    fn crumb(&self) -> String {
        "yaml".into()
    }

    fn hints(&self) -> Vec<Hint> {
        self.pager.hints()
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &AppCtx) -> Vec<Cmd> {
        self.pager.handle_key(key, ctx)
    }

    fn on_msg(&mut self, msg: &crate::msg::Msg, ctx: &AppCtx) -> Vec<Cmd> {
        if let crate::msg::Msg::Fetched {
            result: Ok(crate::cmd::FetchResult::Yaml(text)),
            ..
        } = msg
        {
            self.set_text(text.clone());
            return vec![];
        }
        self.pager.on_msg(msg, ctx)
    }

    fn wants_filter(&self) -> bool {
        self.pager.wants_filter()
    }

    fn set_filter(&mut self, filter: &str) -> Vec<Cmd> {
        self.pager.set_filter(filter)
    }

    fn filter(&self) -> String {
        self.pager.filter()
    }

    fn render(&self, f: &mut Frame, area: Rect, th: &Theme, filter: &str) {
        self.pager.render(f, area, th, filter);
    }
}
