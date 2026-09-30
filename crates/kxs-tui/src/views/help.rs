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

const GLOBAL_KEYS: [(&str, &str); 18] = [
    (":", "command prompt"),
    ("/", "filter (regex; ! inverts, -f fuzzy, -l labels)"),
    ("esc", "clear filter / back"),
    ("?", "help"),
    ("ctrl-a", "aliases"),
    ("ctrl-c, :q", "quit"),
    ("0-9", "namespace favorites (0 = all)"),
    ("j/k, ↑/↓", "move selection"),
    ("g/G, Home/End", "first/last"),
    ("ctrl-f/b, PgUp/PgDn", "page"),
    ("shift-<col initial>", "sort by column"),
    ("ctrl-r", "refresh"),
    ("ctrl-w", "toggle wide columns"),
    ("ctrl-z", "toggle faults only"),
    ("ctrl-e", "toggle header"),
    ("ctrl-g", "toggle breadcrumbs"),
    ("-", "repeat last : command"),
    ("[ / ]", "previous / next : command"),
];

const RESOURCE_KEYS: [(&str, &str); 17] = [
    ("enter", "drill down"),
    ("d", "describe"),
    ("y", "yaml"),
    ("e", "edit"),
    ("l", "logs"),
    ("shift-l", "logs, all containers"),
    ("p", "previous logs"),
    ("s", "shell (Pod) / scale / suspend (CronJob)"),
    ("a", "attach (Pod)"),
    ("c", "copy name"),
    ("n", "copy namespace"),
    ("w", "warp to the row's namespace"),
    ("shift-j", "jump to owner"),
    ("z", "replicasets (Deployment)"),
    ("f", "port-forwards (fullscreen in log/yaml/describe)"),
    ("shift-f", "port forward"),
    ("ctrl-s", "save yaml to file"),
];

const MUTATION_KEYS: [(&str, &str); 6] = [
    ("ctrl-d", "delete (dialog)"),
    ("ctrl-k", "kill — no confirmation"),
    ("r", "restart (workloads) / drain (Node)"),
    ("u", "cordon/uncordon (Node), use (Namespace)"),
    ("t", "trigger (CronJob)"),
    ("h", "rollout history"),
];

const COMMANDS: [(&str, &str); 11] = [
    (":<kind|alias> [ns]", "browse that kind"),
    (":<kind> /filter", "browse, pre-filtered"),
    (":<kind> app=web", "browse, label-selected"),
    (":<kind> @context", "switch context, then browse"),
    (":ctx [name]", "switch context"),
    (":ns [name]", "switch namespace"),
    (":pf", "port-forwards"),
    (":alias", "kind aliases"),
    (":ev, :events", "events"),
    (":theme [id]", "set theme"),
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
        vec![Hint::action("esc", "back")]
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &AppCtx) -> Vec<Cmd> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
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
        let mut lines: Vec<Line> = Vec::new();
        let section = |lines: &mut Vec<Line>, title: &str, rows: &[(&str, &str)]| {
            if !lines.is_empty() {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(Span::styled(title.to_string(), head_style)));
            for (k, d) in rows {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {k:<24}"), key_style),
                    Span::styled(d.to_string(), desc_style),
                ]));
            }
        };
        section(&mut lines, "GLOBAL KEYS", &GLOBAL_KEYS);
        section(&mut lines, "RESOURCE KEYS", &RESOURCE_KEYS);
        section(&mut lines, "MUTATIONS", &MUTATION_KEYS);
        section(&mut lines, "COMMANDS", &COMMANDS);
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
