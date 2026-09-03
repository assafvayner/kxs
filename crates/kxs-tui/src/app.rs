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
    /// One metrics-error flash per session ("missing metrics-server").
    metrics_error_flashed: bool,
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
            metrics_error_flashed: false,
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
            forwards: s
                .forwards
                .iter()
                .map(|f| format!("{}/{}", f.ns, f.pod))
                .collect(),
            forward_rows: s
                .forwards
                .iter()
                .map(|f| {
                    (
                        f.id,
                        f.ns.clone(),
                        f.pod.clone(),
                        f.pod_port,
                        f.local_port,
                        f.started.elapsed().as_secs() as i64,
                    )
                })
                .collect(),
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

    /// Container picker for exec.
    pub fn open_exec_pick(
        &mut self,
        view: ViewId,
        ns: String,
        pod: String,
        options: Vec<(String, String)>,
    ) {
        self.chrome
            .open_pick(format!("Exec({ns}/{pod})"), options, view);
    }

    /// Live theme preview (ThemePicker `j/k`).
    pub fn preview_theme(&mut self, id: &str) {
        let th = crate::theme::get(id);
        if th.id == id {
            self.theme = th;
        }
    }

    /// Confirm-and-undo dialog from the Rollout view.
    pub fn open_confirm_undo(&mut self, view: ViewId, ns: String, name: String, revision: i64) {
        self.chrome.open_confirm(
            format!("Undo {name} to revision {revision}?"),
            format!("{name} will roll back to revision {revision}"),
            view,
            crate::cmd::Mutation::Undo { ns, name, revision },
        );
    }

    /// Open the Chrome container picker; the choice routes back to `view`.
    pub fn open_container_pick(
        &mut self,
        view: ViewId,
        ns: String,
        pod: String,
        options: Vec<(String, String)>,
    ) {
        let title = format!("Containers({ns}/{pod})");
        self.chrome.open_pick(title, options, view);
    }

    /// Execute a `:` command string (used by `--command` and the prompt).
    pub fn exec_command(&mut self, text: &str) -> Vec<Cmd> {
        let text = text.trim_start_matches(':');
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
            Msg::Pod { view, ev } => {
                if !self.on_stack(view) {
                    return vec![];
                }
                self.route(view, &Msg::Pod { view, ev })
            }
            Msg::LogLines { view, pod, lines } => {
                if !self.on_stack(view) {
                    return vec![];
                }
                self.route(view, &Msg::LogLines { view, pod, lines })
            }
            Msg::LogStatus { view, pod, status } => {
                if !self.on_stack(view) {
                    return vec![];
                }
                self.route(view, &Msg::LogStatus { view, pod, status })
            }
            Msg::Metrics { view, pods, nodes } => {
                self.on_metrics(&pods, &nodes);
                if !self.on_stack(view) {
                    return vec![];
                }
                self.route(view, &Msg::Metrics { view, pods, nodes })
            }
            Msg::Mutated { view, m, result } => {
                if !self.on_stack(view) {
                    return vec![];
                }
                match &result {
                    Ok(Some(text)) => self.chrome.flash(text.clone(), false),
                    Ok(None) => {}
                    Err(e) => self.chrome.flash(e.clone(), true),
                }
                let _ = m;
                vec![]
            }
            Msg::ForwardStarted {
                view,
                id,
                local_port,
            } => {
                if !self.on_stack(view) {
                    return vec![];
                }
                self.chrome
                    .flash(format!("forward started on 127.0.0.1:{local_port}"), false);
                let _ = id;
                vec![]
            }
            Msg::Picked { view, choice } => {
                if !self.on_stack(view) {
                    return vec![];
                }
                self.route(view, &Msg::Picked { view, choice })
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

    /// Header CPU/MEM from the node metrics poll; one error flash per session.
    fn on_metrics(
        &mut self,
        pods: &Result<Vec<kxs_cluster::metrics::MetricsRow>, String>,
        nodes: &Result<Vec<kxs_cluster::metrics::NodeMetricsRow>, String>,
    ) {
        if let Err(e) = nodes {
            if !self.metrics_error_flashed {
                self.metrics_error_flashed = true;
                self.chrome.flash(format!("metrics: {e}"), true);
            }
            return;
        }
        let nodes = nodes.as_ref().expect("nodes ok");
        if nodes.is_empty() {
            return; // metrics-server absent: hide the header line, no spam
        }
        let sum =
            |get: fn(&kxs_cluster::metrics::NodeMetricsRow) -> (u64, Option<u64>)| -> (u64, u64) {
                let mut used = 0u64;
                let mut total = 0u64;
                let mut known = false;
                for n in nodes {
                    let (u, t) = get(n);
                    used += u;
                    if let Some(t) = t {
                        total += t;
                        known = true;
                    }
                }
                (used, if known { total } else { 0 })
            };
        let (cpu_used, cpu_total) = sum(|n| (n.cpu_millicores, n.cpu_allocatable_millicores));
        let (mem_used, mem_total) = sum(|n| (n.mem_mib, n.mem_allocatable_mib));
        let fmt = |used: u64, total: u64| -> String {
            match kxs_cluster::utilization::percent(used, Some(total)) {
                Some(p) => format!("{p}%"),
                None => "—".into(),
            }
        };
        self.chrome.cpu_mem = Some(format!(
            "{} / {}",
            fmt(cpu_used, cpu_total),
            fmt(mem_used, mem_total)
        ));
        let _ = pods;
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
                self.metrics_error_flashed = false;
                self.chrome.flash(format!("connected: {context}"), false);
                let view = Box::new(crate::views::pods::PodsView::new(
                    self,
                    self.ctx().namespace,
                ));
                let mut cmds = self.replace_views(vec![view]);
                if let Some(id) = self.views.last().map(|v| v.id()) {
                    let secs = self
                        .config
                        .lock()
                        .map(|c| c.metrics_interval_secs)
                        .unwrap_or(15);
                    cmds.push(Cmd::PollMetrics {
                        view: id,
                        every: std::time::Duration::from_secs(secs.max(1)),
                    });
                }
                cmds
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
        // -1. mutation modals capture everything while open
        if self.chrome.confirm.is_some() {
            return match self.chrome.confirm_key(key) {
                crate::chrome::ConfirmOutcome::Confirmed(action) => {
                    let view = self.chrome.confirm.as_ref().map(|c| c.for_view);
                    self.chrome.close_all_modals();
                    match view {
                        Some(view) => vec![Cmd::Mutate { view, m: *action }],
                        None => vec![],
                    }
                }
                crate::chrome::ConfirmOutcome::Cancelled => {
                    self.chrome.close_all_modals();
                    vec![]
                }
                _ => vec![],
            };
        }
        if self.chrome.input.is_some() {
            return match self.chrome.input_key(key) {
                crate::chrome::InputOutcome::Submitted(value) => {
                    let submitted = self
                        .chrome
                        .input
                        .as_ref()
                        .map(|i| (i.for_view, i.action.clone()));
                    self.chrome.close_all_modals();
                    match submitted {
                        Some((view, crate::chrome::InputAction::Scale { kind, ns, name })) => {
                            match value.trim().parse::<i32>() {
                                Ok(replicas) => vec![Cmd::Mutate {
                                    view,
                                    m: crate::cmd::Mutation::Scale {
                                        kind,
                                        ns,
                                        name,
                                        replicas,
                                    },
                                }],
                                Err(_) => {
                                    self.chrome.flash("replicas must be a number", true);
                                    vec![]
                                }
                            }
                        }
                        None => vec![],
                    }
                }
                crate::chrome::InputOutcome::Cancelled => {
                    self.chrome.close_all_modals();
                    vec![]
                }
                _ => vec![],
            };
        }
        if self.chrome.delete.is_some() {
            return match self.chrome.delete_key(key) {
                crate::chrome::DeleteOutcome::Confirmed => {
                    let dm = self.chrome.delete.take();
                    match dm {
                        Some(dm) => vec![Cmd::Mutate {
                            view: dm.for_view,
                            m: crate::cmd::Mutation::Delete {
                                kind: dm.kind,
                                ns: dm.ns,
                                name: dm.name,
                                propagation: Some(
                                    crate::chrome::PROPAGATIONS[dm.propagation_idx].to_string(),
                                ),
                                force: dm.force,
                            },
                        }],
                        None => vec![],
                    }
                }
                crate::chrome::DeleteOutcome::Cancelled => {
                    self.chrome.close_all_modals();
                    vec![]
                }
                _ => vec![],
            };
        }
        // 0. pick modal captures everything while open
        if self.chrome.pick.is_some() {
            return match self.chrome.pick_key(key) {
                crate::chrome::PickOutcome::Chose(choice) => {
                    let view = self.chrome.pick.as_ref().map(|p| p.for_view);
                    self.chrome.close_pick();
                    match view {
                        Some(view) => self.route(view, &Msg::Picked { view, choice }),
                        None => vec![],
                    }
                }
                crate::chrome::PickOutcome::Cancel => {
                    let view = self.chrome.pick.as_ref().map(|p| p.for_view);
                    self.chrome.close_pick();
                    match view {
                        Some(view) => self.route(view, &Msg::Picked { view, choice: None }),
                        None => vec![],
                    }
                }
                _ => vec![],
            };
        }
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
            KeyCode::Char('?') => {
                let view = Box::new(crate::views::help::HelpView::new(self));
                return self.push_view(view);
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return self.quit_cmds();
            }
            KeyCode::Esc => {
                return self.esc_cascade();
            }
            KeyCode::Char(c @ '0'..='9')
                if !self.top_view().is_some_and(|v| v.handles_digits()) =>
            {
                return self.favorite_key(c);
            }
            _ => {}
        }
        // 3. resource actions handled at app level (need the target + stack push)
        let ctrl_d =
            key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL);
        if matches!(
            key.code,
            KeyCode::Char('d')
                | KeyCode::Char('y')
                | KeyCode::Char('l')
                | KeyCode::Char('L')
                | KeyCode::Enter
                | KeyCode::Char('e')
                | KeyCode::Char('s')
                | KeyCode::Char('r')
                | KeyCode::Char('c')
                | KeyCode::Char('u')
                | KeyCode::Char('t')
                | KeyCode::Char('S')
                | KeyCode::Char('h')
                | KeyCode::Char('x')
                | KeyCode::Char('F')
                | KeyCode::Char('f')
        ) || ctrl_d
        {
            let enter_wanted = self.views.last().is_some_and(|v| v.wants_enter());
            let target = self.views.last().and_then(|v| v.target());
            if let Some(t) = target {
                match key.code {
                    KeyCode::Enter if !enter_wanted => {}
                    KeyCode::Char('d') if !ctrl_d => return self.open_text_view(&t, true),
                    KeyCode::Char('y') => return self.open_text_view(&t, false),
                    KeyCode::Char('l') => return self.open_logs(&t, false),
                    KeyCode::Char('L') => return self.open_logs(&t, true),
                    KeyCode::Enter => return self.open_enter(&t),
                    KeyCode::Char('x')
                        if matches!(t.kind.kind.as_str(), "ConfigMap" | "Secret") =>
                    {
                        let view = Box::new(crate::views::values::ValuesView::new(self, t.clone()));
                        return self.push_view(view);
                    }
                    KeyCode::Char('F') if t.kind.kind == "Pod" => {
                        return self.begin_port_forward(&t);
                    }
                    KeyCode::Char('F') if t.kind.kind == "Service" => {
                        return self.begin_service_forward(&t);
                    }
                    KeyCode::Char('f') => {
                        let view = Box::new(crate::views::forwards::ForwardsView::new(self));
                        return self.push_view(view);
                    }
                    KeyCode::Char('s') if t.kind.kind == "Pod" => {
                        return self.begin_exec(&t);
                    }
                    KeyCode::Char('h') if kxs_cluster::kinds::is_restartable(&t.kind.kind) => {
                        let view =
                            Box::new(crate::views::rollout::RolloutView::new(self, t.clone()));
                        let mut cmds = self.push_view(view);
                        if let Some(id) = self.views.last().map(|v| v.id()) {
                            cmds.push(Cmd::Fetch {
                                view: id,
                                what: crate::cmd::Fetch::RolloutHistory {
                                    ns: t.ns.clone().unwrap_or_default(),
                                    name: t.name.clone(),
                                },
                            });
                        }
                        return cmds;
                    }
                    _ => {
                        // mutating keys — guarded by readonly
                        if let Some(cmds) = self.mutation_key(key, &t) {
                            return cmds;
                        }
                    }
                }
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
                    vec![Cmd::SaveConfig]
                }
                None => {
                    self.chrome.flash(
                        "usage: :theme <id> (picker arrives in a later phase)",
                        false,
                    );
                    vec![]
                }
            },
            "events" | "ev" => {
                let ns = self.ctx().namespace;
                let view = Box::new(crate::views::events::EventsView::new(self, ns));
                self.replace_views(vec![view])
            }
            "metrics" | "top" => {
                let view = Box::new(crate::views::metrics::MetricsView::new(self));
                self.replace_views(vec![view])
            }
            _ => {
                // `<kind|alias> [namespace]` — replace the stack
                let ns = arg.map(String::from);
                if ns.is_some() {
                    self.set_namespace(ns.clone());
                }
                let kinds = self.ctx().kinds;
                match kxs_cluster::command::resolve_kind(&kinds, head) {
                    Some(kind) if kind.kind == "Pod" && kind.group.is_empty() => {
                        let view = Box::new(crate::views::pods::PodsView::new(
                            self,
                            self.ctx().namespace,
                        ));
                        self.replace_views(vec![view])
                    }
                    Some(_) => match crate::views::resources::open(self, head, None) {
                        Some(view) => self.replace_views(vec![view]),
                        None => {
                            self.chrome
                                .flash(format!("unknown command or kind: {head}"), true);
                            vec![]
                        }
                    },
                    None => {
                        self.chrome
                            .flash(format!("unknown command or kind: {head}"), true);
                        vec![]
                    }
                }
            }
        }
    }

    /// shift-f on a Pod: resolve its container ports, then open the forward
    /// flow (single port starts directly; several go through the picker).
    fn begin_port_forward(&mut self, t: &crate::view::Target) -> Vec<Cmd> {
        let view = self.views.last().map(|v| v.id());
        let Some(view) = view else { return vec![] };
        if self.ctx().readonly {
            self.chrome.flash("readonly: port-forward disabled", true);
            return vec![];
        }
        vec![Cmd::Fetch {
            view,
            what: crate::cmd::Fetch::ExecTargets {
                ns: t.ns.clone().unwrap_or_default(),
                pod: t.name.clone(),
            },
        }]
    }

    /// shift-f on a Service: resolve a ready backing pod + port.
    fn begin_service_forward(&mut self, t: &crate::view::Target) -> Vec<Cmd> {
        let view = self.views.last().map(|v| v.id());
        let Some(view) = view else { return vec![] };
        if self.ctx().readonly {
            self.chrome.flash("readonly: port-forward disabled", true);
            return vec![];
        }
        vec![Cmd::Fetch {
            view,
            what: crate::cmd::Fetch::ServiceEndpoint {
                ns: t.ns.clone().unwrap_or_default(),
                name: t.name.clone(),
                port: 80,
            },
        }]
    }

    /// s on a Pod: exec; several containers go through the picker.
    fn begin_exec(&mut self, t: &crate::view::Target) -> Vec<Cmd> {
        let view = self.views.last().map(|v| v.id());
        let Some(view) = view else { return vec![] };
        if self.ctx().readonly {
            self.chrome.flash("readonly: shell disabled", true);
            return vec![];
        }
        vec![Cmd::Fetch {
            view,
            what: crate::cmd::Fetch::ForwardPorts {
                ns: t.ns.clone().unwrap_or_default(),
                pod: t.name.clone(),
            },
        }]
    }

    /// Mutating keys; `None` = not a mutation key (fall through to the view).
    /// Refuses with a flash when readonly.
    fn mutation_key(&mut self, key: KeyEvent, t: &crate::view::Target) -> Option<Vec<Cmd>> {
        let readonly = self.ctx().readonly;
        let view = self.views.last().map(|v| v.id());
        let refuse = |app: &mut Self, what: &str| {
            if readonly {
                app.chrome.flash(format!("readonly: {what} disabled"), true);
                Some(vec![])
            } else {
                None
            }
        };
        match (
            key.code,
            key.modifiers.contains(KeyModifiers::CONTROL),
            t.kind.kind.as_str(),
        ) {
            (KeyCode::Char('e'), _, _) => {
                if let Some(out) = refuse(self, "edit") {
                    return Some(out);
                }
                Some(vec![Cmd::Suspend(crate::cmd::SuspendAction::Edit {
                    kind: t.kind.clone(),
                    ns: t.ns.clone(),
                    name: t.name.clone(),
                })])
            }
            (KeyCode::Char('s'), _, _) if kxs_cluster::kinds::is_scalable(&t.kind.kind) => {
                if let Some(out) = refuse(self, "scale") {
                    return Some(out);
                }
                if let Some(view) = view {
                    self.chrome.open_input(
                        format!("Scale {} to replicas:", t.name),
                        t.desired_replicas
                            .map(|r| r.to_string())
                            .unwrap_or_default(),
                        view,
                        crate::chrome::InputAction::Scale {
                            kind: t.kind.clone(),
                            ns: t.ns.clone().unwrap_or_default(),
                            name: t.name.clone(),
                        },
                    );
                }
                Some(vec![])
            }
            (KeyCode::Char('r'), _, _) if kxs_cluster::kinds::is_restartable(&t.kind.kind) => {
                if let Some(out) = refuse(self, "restart") {
                    return Some(out);
                }
                if let Some(view) = view {
                    self.chrome.open_confirm(
                        format!("Restart {}?", t.name),
                        format!("A rolling restart of {} will be performed", t.name),
                        view,
                        crate::cmd::Mutation::Restart {
                            kind: t.kind.clone(),
                            ns: t.ns.clone().unwrap_or_default(),
                            name: t.name.clone(),
                        },
                    );
                }
                Some(vec![])
            }
            (KeyCode::Char('r'), _, "Node") => {
                if let Some(out) = refuse(self, "drain") {
                    return Some(out);
                }
                if let Some(view) = view {
                    self.chrome.open_confirm(
                        format!("Drain {}?", t.name),
                        "Pods will be evicted from this node".into(),
                        view,
                        crate::cmd::Mutation::Drain {
                            ns: t.ns.clone().unwrap_or_default(),
                            name: t.name.clone(),
                        },
                    );
                }
                Some(vec![])
            }
            (KeyCode::Char('c'), _, "Node") => {
                if let Some(out) = refuse(self, "cordon") {
                    return Some(out);
                }
                view.map(|view| {
                    vec![Cmd::Mutate {
                        view,
                        m: crate::cmd::Mutation::Cordon {
                            ns: t.ns.clone().unwrap_or_default(),
                            name: t.name.clone(),
                            unschedulable: true,
                        },
                    }]
                })
            }
            (KeyCode::Char('u'), _, "Node") => {
                if let Some(out) = refuse(self, "uncordon") {
                    return Some(out);
                }
                view.map(|view| {
                    vec![Cmd::Mutate {
                        view,
                        m: crate::cmd::Mutation::Cordon {
                            ns: t.ns.clone().unwrap_or_default(),
                            name: t.name.clone(),
                            unschedulable: false,
                        },
                    }]
                })
            }
            (KeyCode::Char('t'), _, "CronJob") => {
                if let Some(out) = refuse(self, "trigger") {
                    return Some(out);
                }
                if let Some(view) = view {
                    self.chrome.open_confirm(
                        format!("Trigger {}?", t.name),
                        "A Job will be created from the CronJob template".into(),
                        view,
                        crate::cmd::Mutation::Trigger {
                            ns: t.ns.clone().unwrap_or_default(),
                            name: t.name.clone(),
                        },
                    );
                }
                Some(vec![])
            }
            (KeyCode::Char('S'), _, "CronJob") => {
                if let Some(out) = refuse(self, "suspend toggle") {
                    return Some(out);
                }
                view.map(|view| {
                    vec![Cmd::Mutate {
                        view,
                        m: crate::cmd::Mutation::Suspend {
                            ns: t.ns.clone().unwrap_or_default(),
                            name: t.name.clone(),
                            suspend: !t.suspend.unwrap_or(false),
                        },
                    }]
                })
            }
            (KeyCode::Char('d'), true, _) => {
                if let Some(out) = refuse(self, "delete") {
                    return Some(out);
                }
                if let Some(view) = view {
                    self.chrome.open_delete(
                        format!("Delete {} {}?", t.kind.kind, t.name),
                        view,
                        t.kind.clone(),
                        t.ns.clone().unwrap_or_default(),
                        t.name.clone(),
                    );
                }
                Some(vec![])
            }
            (KeyCode::Char('k'), true, _) => {
                if let Some(out) = refuse(self, "force delete") {
                    return Some(out);
                }
                if let Some(view) = view {
                    self.chrome.open_confirm(
                        format!("Force delete {} {}?", t.kind.kind, t.name),
                        "Grace period 0 (force); propagation Background".into(),
                        view,
                        crate::cmd::Mutation::Delete {
                            kind: t.kind.clone(),
                            ns: t.ns.clone().unwrap_or_default(),
                            name: t.name.clone(),
                            propagation: Some("Background".into()),
                            force: true,
                        },
                    );
                }
                Some(vec![])
            }
            _ => None,
        }
    }

    /// `l` on a Pod (logs, with container picker when several) or on a pod
    /// owner (all pods of the workload).
    fn open_logs(&mut self, target: &crate::view::Target, all_containers: bool) -> Vec<Cmd> {
        let is_pod = target.kind.kind == "Pod";
        let view: Box<dyn View> = if is_pod {
            if let Some(container) = &target.container {
                let mut t = target.clone();
                t.container = Some(container.clone());
                Box::new(crate::views::logs::LogsView::new_with_container(self, t))
            } else {
                Box::new(crate::views::logs::LogsView::new(
                    self,
                    target.clone(),
                    all_containers,
                ))
            }
        } else if kxs_cluster::kinds::is_pod_owner(&target.kind.kind) {
            Box::new(crate::views::logs::LogsView::new_workload(
                self,
                target.clone(),
            ))
        } else {
            self.chrome.flash(
                format!("logs not available for {}", target.kind.kind),
                false,
            );
            return vec![];
        };
        self.push_view(view)
    }

    /// Enter on a Pod → Containers; on a pod owner / CronJob → Pods view
    /// filtered by the workload's selector.
    fn open_enter(&mut self, target: &crate::view::Target) -> Vec<Cmd> {
        if target.kind.kind == "Pod" {
            let view = Box::new(crate::views::containers::ContainersView::new(
                self,
                target.clone(),
            ));
            let mut cmds = self.push_view(view);
            if let Some(id) = self.views.last().map(|v| v.id()) {
                cmds.push(Cmd::Fetch {
                    view: id,
                    what: crate::cmd::Fetch::Containers {
                        ns: target.ns.clone().unwrap_or_default(),
                        pod: target.name.clone(),
                    },
                });
            }
            return cmds;
        }
        if kxs_cluster::kinds::views_pods(&target.kind.kind) {
            let ns = target.ns.clone();
            let view = Box::new(crate::views::pods::PodsView::new_with_pending_selector(
                self, ns,
            ));
            let mut cmds = self.push_view(view);
            if let Some(id) = self.views.last().map(|v| v.id()) {
                cmds.push(Cmd::Fetch {
                    view: id,
                    what: crate::cmd::Fetch::WorkloadSelector {
                        kind: target.kind.clone(),
                        ns: target.ns.clone().unwrap_or_default(),
                        name: target.name.clone(),
                    },
                });
            }
            return cmds;
        }
        vec![]
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

        let readonly = self.config.lock().map(|c| c.readonly).unwrap_or(false);
        let hints: Vec<crate::view::Hint> = self
            .views
            .last()
            .map(|v| v.hints())
            .unwrap_or_default()
            .into_iter()
            .filter(|h| !readonly || !h.mutating)
            .collect();
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

        if self.chrome.pick.is_some() {
            self.chrome.render_pick(f, f.area(), &self.theme);
        }
        if self.chrome.confirm.is_some() {
            self.chrome.render_confirm(f, f.area(), &self.theme);
        }
        if self.chrome.input.is_some() {
            self.chrome.render_input(f, f.area(), &self.theme);
        }
        if self.chrome.delete.is_some() {
            self.chrome.render_delete(f, f.area(), &self.theme);
        }
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
