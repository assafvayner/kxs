//! The Elm-style core: `App::update(Msg) -> Vec<Cmd>` is the only place state
//! changes; `render` never mutates.

use std::sync::{Arc, Mutex};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::Frame;

use kxs_cluster::command as cluster_command;

use crate::chrome::{Chrome, PromptKind, PromptOutcome};
use crate::cmd::Cmd;
use crate::config::Config;
use crate::ctx::AppCtx;
use crate::msg::Msg;
use crate::sessions::Shared;
use crate::theme::Theme;
use crate::view::{View, ViewId};

pub struct App {
    pub views: Vec<Box<dyn View>>,
    pub chrome: Chrome,
    pub sessions: Shared,
    pub config: Arc<Mutex<Config>>,
    pub theme: Theme,
    next_view_id: ViewId,
}

impl App {
    pub fn new(sessions: Shared, config: Arc<Mutex<Config>>, theme: Theme) -> Self {
        let chrome = Chrome {
            size: (100, 30),
            ..Default::default()
        };
        App {
            views: Vec::new(),
            chrome,
            sessions,
            config,
            theme,
            next_view_id: 1,
        }
    }

    pub fn alloc_id(&mut self) -> ViewId {
        let id = self.next_view_id;
        self.next_view_id += 1;
        id
    }

    pub fn push_view(&mut self, mut view: Box<dyn View>) -> Vec<Cmd> {
        // a freshly pushed view may need to start its watch right away
        let cmds = view.on_msg(&Msg::Tick, &self.ctx());
        self.views.push(view);
        self.sync_chrome();
        cmds
    }

    /// Replaces the whole stack (connect, `:kind` navigation).
    pub fn replace_views(&mut self, views: Vec<Box<dyn View>>) -> Vec<Cmd> {
        let mut cmds: Vec<Cmd> = self.views.iter_mut().flat_map(|v| v.on_pop()).collect();
        self.views.clear();
        for v in views {
            cmds.extend(self.push_view(v));
        }
        cmds
    }

    pub fn pop_view(&mut self) -> Vec<Cmd> {
        if self.views.len() <= 1 {
            return vec![];
        }
        let mut view = self.views.pop().expect("len > 1");
        let cmds = view.on_pop();
        self.sync_chrome();
        cmds
    }

    fn sync_chrome(&mut self) {
        let s = self.sessions.lock().expect("sessions lock");
        if let Some(active) = &s.active {
            self.chrome.context = active.name.clone();
            self.chrome.version = active.version.clone();
            self.chrome.namespace = active.namespace.clone();
            let summary = s
                .store
                .contexts()
                .into_iter()
                .find(|c| c.name == active.name);
            if let Some(sum) = summary {
                self.chrome.cluster = sum.cluster;
                self.chrome.user = sum.user;
            }
        }
        // favorites from the per-context config section
        if let Some(active) = &s.active {
            let cfg = self.config.lock().expect("config lock");
            self.chrome.favorites = cfg
                .contexts
                .get(&active.name)
                .map(|c| c.favorites.clone())
                .unwrap_or_default();
        }
    }

    pub fn ctx(&self) -> AppCtx {
        let s = self.sessions.lock().expect("sessions lock");
        let readonly = self.config.lock().map(|c| c.readonly).unwrap_or(false);
        AppCtx {
            namespace: s.active.as_ref().and_then(|a| a.namespace.clone()),
            kinds: s.active_kinds(),
            present: s.active_present(),
            readonly,
            size: self.chrome.size,
        }
    }

    /// Set the active namespace (`0`–`9` favorites, Namespaces view).
    pub fn set_namespace(&mut self, ns: Option<String>) {
        let mut s = self.sessions.lock().expect("sessions lock");
        if let Some(active) = &mut s.active {
            active.namespace = ns;
        }
        drop(s);
        self.sync_chrome();
    }

    /// Push a namespace onto the per-context favorites (MRU, max 9).
    pub fn record_favorite(&mut self, ns: Option<String>) {
        let Some(name) = ns else { return };
        let context = self.chrome.context.clone();
        if context.is_empty() {
            return;
        }
        let mut cfg = self.config.lock().expect("config lock");
        let entry = cfg.contexts.entry(context).or_default();
        if let Some(pos) = entry.favorites.iter().position(|f| *f == name) {
            entry.favorites.remove(pos);
        }
        entry.favorites.insert(0, name);
        entry.favorites.truncate(9);
        drop(cfg);
        self.sync_chrome();
    }

    /// Execute a `:` command string (used by `--command` and the prompt).
    pub fn exec_command(&mut self, text: &str) -> Vec<Cmd> {
        self.handle_command(text)
    }

    pub fn set_theme(&mut self, id: &str) {
        let th = crate::theme::get(id);
        if th.id != id {
            self.chrome.flash(format!("unknown theme \"{id}\""), true);
            return;
        }
        self.theme = th;
        if let Ok(mut cfg) = self.config.lock() {
            cfg.theme = Some(id.to_string());
        }
        self.chrome.flash(format!("theme: {id}"), false);
    }

    /// Routes a message; returns side-effect commands for the runtime.
    pub fn update(&mut self, msg: Msg) -> Vec<Cmd> {
        match msg {
            Msg::Key(k) => self.handle_key(k),
            Msg::Resize(w, h) => {
                self.chrome.size = (w, h);
                vec![]
            }
            Msg::Tick => {
                self.chrome.tick();
                self.sync_chrome();
                let ctx = self.ctx();
                let mut cmds = vec![];
                for v in &mut self.views {
                    cmds.extend(v.on_msg(&Msg::Tick, &ctx));
                }
                cmds
            }
            Msg::Started { view, handle } => {
                if !self.on_stack(view) {
                    return vec![]; // late result from a popped view
                }
                let ctx = self.ctx();
                for v in &mut self.views {
                    if v.id() == view {
                        return v.on_started(handle, &ctx);
                    }
                }
                vec![]
            }
            Msg::Table { view, ev } => {
                if !self.on_stack(view) {
                    return vec![]; // late result from a popped view
                }
                self.route(view, &Msg::Table { view, ev })
            }
            Msg::Fetched { view, result } => {
                if !self.on_stack(view) {
                    return vec![];
                }
                if let Err(text) = &result {
                    self.chrome.flash(text.clone(), true);
                }
                self.route(view, &Msg::Fetched { view, result })
            }
            Msg::Pinged { context, result } => {
                let ctx = self.ctx();
                let mut cmds = vec![];
                for v in &mut self.views {
                    cmds.extend(v.on_msg(
                        &Msg::Pinged {
                            context: context.clone(),
                            result: result.clone(),
                        },
                        &ctx,
                    ));
                }
                cmds
            }
            Msg::Connected { context, result } => self.on_connected(&context, result),
            Msg::Error { view, text } => {
                if let Some(id) = view {
                    if !self.on_stack(id) {
                        return vec![];
                    }
                    return self.route(id, &Msg::Error { view, text });
                }
                self.chrome.flash(text, true);
                vec![]
            }
            Msg::KubeconfigChanged => {
                let mut s = self.sessions.lock().expect("sessions lock");
                let warnings = s.store.reload();
                drop(s);
                for w in warnings {
                    self.chrome.flash(w, true);
                }
                self.chrome.flash("kubeconfig reloaded", false);
                vec![]
            }
        }
    }

    fn on_stack(&self, id: ViewId) -> bool {
        self.views.iter().any(|v| v.id() == id)
    }

    fn route(&mut self, id: ViewId, msg: &Msg) -> Vec<Cmd> {
        let ctx = self.ctx();
        for v in &mut self.views {
            if v.id() == id {
                return v.on_msg(msg, &ctx);
            }
        }
        vec![]
    }

    fn on_connected(&mut self, context: &str, result: Result<String, String>) -> Vec<Cmd> {
        match result {
            Ok(_version) => {
                self.sync_chrome();
                self.chrome.flash(format!("connected: {context}"), false);
                match crate::views::resources::open(self, "pods", None) {
                    Some(view) => self.replace_views(vec![view]),
                    None => vec![],
                }
            }
            Err(text) => {
                self.chrome
                    .flash(format!("connect {context}: {text}"), true);
                if self.views.is_empty() {
                    let view = Box::new(crate::views::contexts::ContextsView::new(self));
                    self.push_view(view)
                } else {
                    vec![]
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Vec<Cmd> {
        // 1. prompt captures everything while open
        if self.chrome.prompt.is_some() {
            return self.prompt_key(key);
        }
        // 2. global keys
        match key.code {
            KeyCode::Char(':') => {
                self.chrome.open_prompt(PromptKind::Command);
                return vec![];
            }
            KeyCode::Char('/') => {
                if self.top_view().is_some_and(|v| v.wants_filter()) {
                    self.chrome.open_prompt(PromptKind::Filter);
                }
                return vec![];
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return self.quit_cmds();
            }
            KeyCode::Esc => {
                return self.esc_cascade();
            }
            KeyCode::Char(c @ '0'..='9') => {
                return self.favorite_key(c);
            }
            _ => {}
        }
        // 3. resource actions handled at app level (need the target + stack push)
        if matches!(key.code, KeyCode::Char('d') | KeyCode::Char('y')) {
            let target = self.views.last().and_then(|v| v.target());
            if let Some(t) = target {
                return self.open_text_view(&t, key.code == KeyCode::Char('d'));
            }
        }
        // 4. top view
        let ctx = self.ctx();
        match self.views.last_mut() {
            Some(v) => v.handle_key(key, &ctx),
            None => vec![],
        }
    }

    /// `d` → Describe, `y` → YAML: push the view and fetch its text.
    fn open_text_view(&mut self, target: &crate::view::Target, describe: bool) -> Vec<Cmd> {
        let (view, what) = if describe {
            let v = crate::views::describe::DescribeView::new(self, target);
            let what = crate::cmd::Fetch::Describe {
                kind: target.kind.clone(),
                ns: target.ns.clone(),
                name: target.name.clone(),
            };
            (Box::new(v) as Box<dyn View>, what)
        } else {
            let v = crate::views::yaml::YamlView::new(self, target);
            let what = crate::cmd::Fetch::Yaml {
                kind: target.kind.clone(),
                ns: target.ns.clone(),
                name: target.name.clone(),
            };
            (Box::new(v) as Box<dyn View>, what)
        };
        let mut cmds = self.push_view(view);
        if let Some(id) = self.views.last().map(|v| v.id()) {
            cmds.push(Cmd::Fetch { view: id, what });
        }
        cmds
    }

    fn top_view(&mut self) -> Option<&mut Box<dyn View>> {
        self.views.last_mut()
    }

    fn esc_cascade(&mut self) -> Vec<Cmd> {
        if let Some(v) = self.top_view() {
            if !v.filter().is_empty() {
                v.set_filter("");
                return vec![];
            }
        }
        self.pop_view()
    }

    fn favorite_key(&mut self, c: char) -> Vec<Cmd> {
        if c == '0' {
            self.set_namespace(None);
            return vec![];
        }
        let idx = c as usize - '1' as usize;
        match self.chrome.favorites.get(idx) {
            Some(ns) => {
                let ns = ns.clone();
                self.set_namespace(Some(ns));
                vec![]
            }
            None => {
                self.chrome
                    .flash(format!("no namespace favorite <{c}>"), false);
                vec![]
            }
        }
    }

    fn prompt_key(&mut self, key: KeyEvent) -> Vec<Cmd> {
        match self.chrome.prompt_key(key) {
            PromptOutcome::Ignored | PromptOutcome::Edited => vec![],
            PromptOutcome::Cancel => {
                self.chrome.close_prompt();
                vec![]
            }
            PromptOutcome::Submit(text) => {
                let kind = self.chrome.prompt.as_ref().map(|p| p.kind);
                self.chrome.close_prompt();
                match kind {
                    Some(PromptKind::Command) => self.handle_command(&text),
                    Some(PromptKind::Filter) => {
                        if let Some(v) = self.top_view() {
                            v.set_filter(&text);
                        }
                        vec![]
                    }
                    None => vec![],
                }
            }
        }
    }

    /// `:` command parsing (Phase 2 subset of the spec's command table).
    fn handle_command(&mut self, text: &str) -> Vec<Cmd> {
        let text = text.trim();
        if text.is_empty() {
            return vec![];
        }
        let mut parts = text.split_whitespace();
        let head = parts.next().unwrap_or_default();
        let arg = parts.next();
        match head {
            "q" | "quit" => self.quit_cmds(),
            "ctx" => match arg {
                Some(name) => vec![Cmd::Connect {
                    context: name.to_string(),
                }],
                None => {
                    let view = Box::new(crate::views::contexts::ContextsView::new(self));
                    self.push_view(view)
                }
            },
            "ns" => match arg {
                Some(ns) => {
                    self.set_namespace(Some(ns.to_string()));
                    vec![]
                }
                None => {
                    let view = Box::new(crate::views::namespaces::NamespacesView::new(self));
                    self.push_view(view)
                }
            },
            "help" => {
                let view = Box::new(crate::views::help::HelpView::new(self));
                self.push_view(view)
            }
            "theme" => match arg {
                Some(id) => {
                    self.set_theme(id);
                    vec![]
                }
                None => {
                    self.chrome.flash(
                        "usage: :theme <id> (picker arrives in a later phase)",
                        false,
                    );
                    vec![]
                }
            },
            _ => {
                // `<kind|alias> [namespace]` — replace the stack
                let ns = arg.map(String::from);
                if ns.is_some() {
                    self.set_namespace(ns.clone());
                }
                match crate::views::resources::open(self, head, None) {
                    Some(view) => self.replace_views(vec![view]),
                    None => {
                        self.chrome
                            .flash(format!("unknown command or kind: {head}"), true);
                        vec![]
                    }
                }
            }
        }
    }

    fn quit_cmds(&mut self) -> Vec<Cmd> {
        let mut cmds: Vec<Cmd> = self.views.iter_mut().flat_map(|v| v.on_pop()).collect();
        cmds.push(Cmd::Quit);
        cmds
    }

    /// Fuzzy completion candidates for the `:` prompt.
    pub fn completions(&self, input: &str) -> Vec<String> {
        let query = input
            .trim_start_matches(':')
            .split_whitespace()
            .next()
            .unwrap_or("");
        if query.is_empty() {
            return vec![
                "ctx".into(),
                "ns".into(),
                "theme".into(),
                "help".into(),
                "q".into(),
            ];
        }
        let s = self.sessions.lock().expect("sessions lock");
        let kinds: Vec<kxs_cluster::discovery::ResourceKind> = s.active_kinds().as_ref().clone();
        let present = s.active_present();
        drop(s);
        let visible: Vec<kxs_cluster::discovery::ResourceKind> =
            cluster_command::visible_kinds(&kinds, present.as_deref())
                .into_iter()
                .cloned()
                .collect();
        let mut out: Vec<String> = Vec::new();
        if "ctx".starts_with(query) {
            out.push("ctx".into());
        }
        if "ns".starts_with(query) {
            out.push("ns".into());
        }
        if "help".starts_with(query) {
            out.push("help".into());
        }
        for k in cluster_command::fuzzy_kinds(&visible, query) {
            out.push(k.plural);
        }
        out
    }

    /// Full-frame render: header, body (top view), footer.
    pub fn render(&self, f: &mut Frame) {
        let header_h = Chrome::header_height(f.area().width);
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(header_h),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(f.area());

        let hints = self.views.last().map(|v| v.hints()).unwrap_or_default();
        self.chrome.render_header(f, header, &self.theme, &hints);

        // prompt replaces the title row; the view keeps the rest
        let (title_row, view_area) = if self.chrome.prompt.is_some() {
            let [title, rest] =
                Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(body);
            self.chrome.render_prompt(f, title, &self.theme);
            (Some(title), rest)
        } else {
            (None, body)
        };
        let _ = title_row;

        if let Some(v) = self.views.last() {
            v.render(f, view_area, &self.theme, &v.filter());
        }

        let crumbs: Vec<String> = self.views.iter().map(|v| v.crumb()).collect();
        self.chrome.render_footer(f, footer, &self.theme, &crumbs);
    }

    pub fn body_area(&self, full: Rect) -> Rect {
        let header_h = Chrome::header_height(full.width);
        Rect {
            x: full.x,
            y: full.y + header_h,
            width: full.width,
            height: full.height.saturating_sub(header_h + 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::Sessions;
    use kxs_core::kubeconfig::store::KubeconfigStore;

    fn test_app() -> App {
        let sessions: Shared = Arc::new(Mutex::new(Sessions::new(
            KubeconfigStore::load_tolerant(vec![]).0,
        )));
        App::new(
            sessions,
            Arc::new(Mutex::new(Config::default())),
            crate::theme::get(crate::theme::DEFAULT_ID),
        )
    }

    #[test]
    fn esc_cascade_prompt_then_pop() {
        let mut app = test_app();
        let view = Box::new(crate::views::help::HelpView::new(&mut app));
        app.push_view(view);
        let view = Box::new(crate::views::help::HelpView::new(&mut app));
        app.push_view(view);
        // open the command prompt
        app.update(Msg::Key(KeyEvent::from(KeyCode::Char(':'))));
        assert!(app.chrome.prompt.is_some());
        // Esc closes the prompt, does not pop the view
        app.update(Msg::Key(KeyEvent::from(KeyCode::Esc)));
        assert!(app.chrome.prompt.is_none());
        assert_eq!(app.views.len(), 2);
        // a further Esc pops the view
        let len = app.views.len();
        app.update(Msg::Key(KeyEvent::from(KeyCode::Esc)));
        assert_eq!(app.views.len(), len - 1);
    }

    #[test]
    fn late_messages_from_popped_views_are_dropped() {
        let mut app = test_app();
        let view = Box::new(crate::views::help::HelpView::new(&mut app));
        app.push_view(view);
        let id = app.views[0].id();
        app.pop_view();
        // no panic, no routing — the view is gone
        let cmds = app.update(Msg::Table {
            view: id,
            ev: kxs_cluster::resources::TableEvent::Status {
                state: "ok".into(),
                message: None,
            },
        });
        assert!(cmds.is_empty());
    }

    #[test]
    fn never_pops_the_last_view() {
        let mut app = test_app();
        let view = Box::new(crate::views::help::HelpView::new(&mut app));
        app.push_view(view);
        app.update(Msg::Key(KeyEvent::from(KeyCode::Esc)));
        assert_eq!(app.views.len(), 1);
    }

    #[test]
    fn quit_stops_streams_and_returns_quit() {
        let mut app = test_app();
        let view = Box::new(crate::views::help::HelpView::new(&mut app));
        app.push_view(view);
        let cmds = app.update(Msg::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert!(matches!(cmds.last(), Some(Cmd::Quit)));
    }

    #[test]
    fn command_parsing() {
        let mut app = test_app();
        let cmds = app.handle_command("q");
        assert!(matches!(cmds.last(), Some(Cmd::Quit)));
        let cmds = app.handle_command("ctx kind-local");
        assert!(matches!(cmds.first(), Some(Cmd::Connect { context }) if context == "kind-local"));
        // unknown command flashes, does not panic
        app.handle_command("bogus");
        assert!(app.chrome.flash.is_some());
    }

    #[test]
    fn readonly_flag_reaches_ctx() {
        let sessions: Shared = Arc::new(Mutex::new(Sessions::new(
            KubeconfigStore::load_tolerant(vec![]).0,
        )));
        let cfg = Config {
            readonly: true,
            ..Default::default()
        };
        let app = App::new(
            sessions,
            Arc::new(Mutex::new(cfg)),
            crate::theme::get(crate::theme::DEFAULT_ID),
        );
        assert!(app.ctx().readonly);
    }
}
