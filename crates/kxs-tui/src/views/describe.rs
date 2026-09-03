//! kubectl-format Describe view (plain text pager).

use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;

use crate::cmd::Cmd;
use crate::theme::Theme;
use crate::view::{Hint, Target, View};
use crate::views::pager::Pager;
use crate::AppCtx;

pub struct DescribeView {
    pager: Pager,
}

impl DescribeView {
    pub fn new(app: &mut crate::app::App, target: &Target) -> Self {
        let label = format!("Describe({}/{})", target.kind.kind, target.name);
        DescribeView {
            pager: Pager::new(app, label, ""),
        }
    }

    pub fn set_text(&mut self, text: String) {
        self.pager.set_text(text);
    }
}

impl View for DescribeView {
    fn id(&self) -> u64 {
        self.pager.id()
    }

    fn title(&self) -> String {
        self.pager.title()
    }

    fn crumb(&self) -> String {
        "describe".into()
    }

    fn hints(&self) -> Vec<Hint> {
        self.pager.hints()
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &AppCtx) -> Vec<Cmd> {
        self.pager.handle_key(key, ctx)
    }

    fn on_msg(&mut self, msg: &crate::msg::Msg, ctx: &AppCtx) -> Vec<Cmd> {
        if let crate::msg::Msg::Fetched {
            result: Ok(crate::cmd::FetchResult::Describe(text)),
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
