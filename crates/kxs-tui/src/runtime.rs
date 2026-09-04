//! The event loop: consumes `Msg`s, runs `Cmd`s as tokio tasks, redraws.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event};
use ratatui::Terminal;
use tokio::sync::mpsc;

use kxs_cluster::bridge;
use kxs_cluster::discovery;
use kxs_cluster::logs::run_log_stream;
use kxs_cluster::pods::run_pod_watch;
use kxs_cluster::resources::{get_yaml, run_table_watch};
use kxs_cluster::session;

use crate::app::App;
use crate::cmd::SuspendAction;
use crate::cmd::{Cmd, Fetch, FetchResult, StopHandle};
use crate::config::{self, Config};
use crate::msg::Msg;
use crate::sessions::Shared;
use crate::terminal;

/// Connects one context: kubeconfig yaml → session → ping → discovery.
/// On success the session, kinds, and namespace are recorded in `sessions`.
pub async fn connect_one(
    sessions: &Shared,
    config: &Arc<Mutex<Config>>,
    context: &str,
) -> Result<String, String> {
    let (yaml, ctx_namespace) = {
        let s = sessions.lock().expect("sessions lock");
        let yaml = bridge::kubeconfig_yaml_for_context(&s.store, context)?;
        let ns = s
            .store
            .contexts()
            .into_iter()
            .find(|c| c.name == context)
            .and_then(|c| c.namespace);
        (yaml, ns)
    };
    let sess = session::connect(&yaml, context).await?;
    let version = session::ping(&sess, Duration::from_secs(10)).await?;
    let kinds = discovery::discover(sess.client.clone()).await?;
    let namespace = {
        let cfg = config.lock().expect("config lock");
        cfg.contexts
            .get(context)
            .and_then(|c| c.namespace.clone())
            .or(ctx_namespace)
    };
    let mut s = sessions.lock().expect("sessions lock");
    s.map.insert(context.to_string(), Arc::new(sess));
    s.kinds.insert(context.to_string(), Arc::new(kinds));
    s.active = Some(crate::sessions::ActiveContext {
        name: context.to_string(),
        namespace,
        version: version.clone(),
    });
    Ok(version)
}

/// Reachability ping for one context; does not disturb the active session.
async fn ping_one(sessions: &Shared, context: &str) -> Result<String, String> {
    let yaml = {
        let s = sessions.lock().expect("sessions lock");
        bridge::kubeconfig_yaml_for_context(&s.store, context)?
    };
    let sess = session::connect(&yaml, context).await?;
    session::ping(&sess, Duration::from_secs(5)).await
}

/// Writes a resource's YAML under the dump directory, k9s' `ctrl-s`.
/// Returns the path written.
async fn save_resource(
    sessions: &Shared,
    kind: &kxs_cluster::discovery::ResourceKind,
    ns: Option<&str>,
    name: &str,
) -> Result<String, String> {
    let (sess, context) = {
        let s = sessions.lock().expect("sessions lock");
        let sess = s.active_session().ok_or("not connected")?;
        let context = s
            .active
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_default();
        (sess, context)
    };
    let yaml = get_yaml(
        sess.client.clone(),
        &kind.group,
        &kind.version,
        &kind.kind,
        &kind.plural,
        ns,
        name,
    )
    .await?;
    let dir = config::dump_dir()?.join(sanitize(&context));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stamp = kxs_cluster::clock::now_ms() / 1000;
    let file = dir.join(format!(
        "{}-{}-{}.yaml",
        sanitize(&kind.plural),
        sanitize(name),
        stamp
    ));
    std::fs::write(&file, yaml).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(file.display().to_string())
}

/// Path-safe form of a context / resource name.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

async fn fetch(sessions: &Shared, what: Fetch) -> Result<FetchResult, String> {
    let sess = {
        let s = sessions.lock().expect("sessions lock");
        s.active_session().ok_or("not connected")?
    };
    match what {
        Fetch::Yaml { kind, ns, name } => Ok(FetchResult::Yaml(
            get_yaml(
                sess.client.clone(),
                &kind.group,
                &kind.version,
                &kind.kind,
                &kind.plural,
                ns.as_deref(),
                &name,
            )
            .await?,
        )),
        Fetch::Describe { kind, ns, name } => Ok(FetchResult::Describe(
            kxs_cluster::describe::describe(sess.client.clone(), &kind, ns.as_deref(), &name)
                .await?,
        )),
        Fetch::Namespaces => Ok(FetchResult::Namespaces(session::namespaces(&sess).await?)),
        Fetch::Containers { ns, pod } => Ok(FetchResult::Containers(
            kxs_cluster::pods::list_container_info(sess.client.clone(), &ns, &pod).await?,
        )),
        Fetch::WorkloadSelector { kind, ns, name } => Ok(FetchResult::Selector(
            kxs_cluster::workloads::workload_selector(
                sess.client.clone(),
                &kind.group,
                &kind.version,
                &kind.kind,
                &kind.plural,
                &ns,
                &name,
            )
            .await?,
        )),
        Fetch::WorkloadPods { kind, ns, name } => Ok(FetchResult::PodNames(
            kxs_cluster::workloads::workload_pods(
                sess.client.clone(),
                &kind.group,
                &kind.version,
                &kind.kind,
                &kind.plural,
                &ns,
                &name,
            )
            .await?,
        )),
        Fetch::RolloutHistory { ns, name } => Ok(FetchResult::Rollout(
            kxs_cluster::workloads::rollout_history(sess.client.clone(), &ns, &name).await?,
        )),
        Fetch::ConfigValues { ns, name, kind } => Ok(FetchResult::Values(
            kxs_cluster::workloads::config_values(sess.client.clone(), &ns, &name, &kind).await?,
        )),
        Fetch::ExecTargets { ns, pod } => Ok(FetchResult::ExecContainers {
            ns: ns.clone(),
            pod: pod.clone(),
            infos: kxs_cluster::pods::list_container_info(sess.client.clone(), &ns, &pod).await?,
        }),
        Fetch::AttachTargets { ns, pod } => Ok(FetchResult::AttachContainers {
            ns: ns.clone(),
            pod: pod.clone(),
            infos: kxs_cluster::pods::list_container_info(sess.client.clone(), &ns, &pod).await?,
        }),
        Fetch::Owner { kind, ns, name } => {
            let yaml = get_yaml(
                sess.client.clone(),
                &kind.group,
                &kind.version,
                &kind.kind,
                &kind.plural,
                ns.as_deref(),
                &name,
            )
            .await?;
            Ok(FetchResult::Owner(kxs_cluster::resources::owner_reference(
                &yaml,
            )))
        }
        Fetch::ForwardPorts { ns, pod } => Ok(FetchResult::ForwardPorts {
            ns: ns.clone(),
            pod: pod.clone(),
            choices: kxs_cluster::containers::port_choices(
                &kxs_cluster::pods::list_container_info(sess.client.clone(), &ns, &pod).await?,
            ),
        }),
        Fetch::ServiceEndpoint { ns, name, port } => {
            let (pod, container_port) = kxs_cluster::workloads::resolve_service_endpoint(
                sess.client.clone(),
                &ns,
                &name,
                port,
            )
            .await?;
            Ok(FetchResult::Endpoint(pod, container_port))
        }
    }
}

pub struct Runtime {
    tx: mpsc::UnboundedSender<Msg>,
    sessions: Shared,
    config: Arc<Mutex<Config>>,
    metrics_poller: Option<tokio::task::JoinHandle<()>>,
    /// Terminal event pump: one dedicated reader thread for the whole
    /// process lifetime. While a suspended child (exec) owns the terminal
    /// the thread is parked — it makes no tty syscalls, so it cannot steal
    /// the child's keystrokes, and no stale crossterm reader lingers to
    /// fight the next one over the tty.
    pump_pause: Arc<AtomicBool>,
    pump_parked: Arc<AtomicBool>,
    pump_started: Arc<AtomicBool>,
}

impl Runtime {
    pub fn new(
        tx: mpsc::UnboundedSender<Msg>,
        sessions: Shared,
        config: Arc<Mutex<Config>>,
    ) -> Self {
        Runtime {
            tx,
            sessions,
            config,
            metrics_poller: None,
            pump_pause: Arc::new(AtomicBool::new(false)),
            pump_parked: Arc::new(AtomicBool::new(false)),
            pump_started: Arc::new(AtomicBool::new(false)),
        }
    }

    /// (Re)activate the event pump. Spawns the single reader thread once;
    /// later calls just unpark it after a suspend round-trip.
    fn start_event_pump(&mut self) {
        if self.pump_started.swap(false, Ordering::SeqCst) {
            // thread already exists: unpark it
            self.pump_started.store(true, Ordering::SeqCst);
            self.pump_pause.store(false, Ordering::SeqCst);
            return;
        }
        self.pump_started.store(true, Ordering::SeqCst);
        let tx = self.tx.clone();
        let pause = self.pump_pause.clone();
        let parked = self.pump_parked.clone();
        pause.store(false, Ordering::SeqCst);
        std::thread::spawn(move || loop {
            if pause.load(Ordering::SeqCst) {
                parked.store(true, Ordering::SeqCst);
                while pause.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                parked.store(false, Ordering::SeqCst);
                continue;
            }
            match event::poll(Duration::from_millis(50)) {
                Ok(true) => match event::read() {
                    Ok(Event::Key(k)) => {
                        if tx.send(Msg::Key(k)).is_err() {
                            break;
                        }
                    }
                    Ok(Event::Resize(w, h)) => {
                        let _ = tx.send(Msg::Resize(w, h));
                    }
                    Ok(_) => {}
                    Err(_) => break,
                },
                Ok(false) => {}
                Err(_) => break,
            }
        });
    }

    /// Park the pump thread: no tty reads while a suspended child (exec)
    /// owns the terminal, so the child's keystrokes are never stolen and no
    /// stale reader lingers to fight the next one over the tty. Waits
    /// (bounded) until the thread has actually parked.
    fn stop_event_pump(&mut self) {
        if !self.pump_started.load(Ordering::SeqCst) {
            return;
        }
        self.pump_pause.store(true, Ordering::SeqCst);
        for _ in 0..50 {
            if self.pump_parked.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Runs until `Cmd::Quit` or the message channel closes. The terminal is
    /// owned here so nothing else renders.
    pub async fn run(
        &mut self,
        app: &mut App,
        mut rx: mpsc::UnboundedReceiver<Msg>,
        screen: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
        pre_cmds: Vec<Cmd>,
    ) -> Result<(), String> {
        self.start_event_pump();
        for cmd in pre_cmds {
            if self.execute(cmd, app).await? {
                return Ok(());
            }
        }
        screen
            .draw(|f| app.render(f))
            .map_err(|e| format!("draw: {e}"))?;
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            let msg = tokio::select! {
                m = rx.recv() => match m {
                    Some(m) => m,
                    None => return Ok(()),
                },
                _ = tick.tick() => Msg::Tick,
            };
            for cmd in app.update(msg) {
                if let Cmd::Suspend(action) = cmd {
                    // handled inline: leave raw mode, run on the real
                    // terminal, restore, force a full redraw
                    // stop consuming the terminal: the child owns it now
                    self.stop_event_pump();
                    terminal::restore();
                    let client = {
                        let s = self.sessions.lock().expect("sessions lock");
                        s.active_session().map(|sess| sess.client.clone())
                    };
                    let result = match client {
                        Some(client) => match action {
                            SuspendAction::Edit { kind, ns, name } => {
                                crate::suspend::run_edit(client, kind, ns, name).await
                            }
                            SuspendAction::Exec { ns, pod, container } => {
                                let size = crossterm::terminal::size().unwrap_or((80, 24));
                                let (cols, rows) = size;
                                crate::suspend::run_exec(
                                    client,
                                    &ns,
                                    &pod,
                                    container.as_deref(),
                                    cols,
                                    rows,
                                )
                                .await
                            }
                            SuspendAction::Attach { ns, pod, container } => {
                                let size = crossterm::terminal::size().unwrap_or((80, 24));
                                let (cols, rows) = size;
                                crate::suspend::run_attach(
                                    client,
                                    &ns,
                                    &pod,
                                    container.as_deref(),
                                    cols,
                                    rows,
                                )
                                .await
                            }
                        },
                        None => Err("not connected".into()),
                    };
                    terminal::enter().ok();
                    // Restart the pump thread, then discard anything it
                    // (or the kernel tty buffer) picked up during the
                    // suspend window so none of it replays into the TUI.
                    self.start_event_pump();
                    while crossterm::event::poll(std::time::Duration::ZERO).unwrap_or(false) {
                        let _ = crossterm::event::read();
                    }
                    while let Ok(m) = rx.try_recv() {
                        if matches!(m, Msg::Key(_)) {
                            continue;
                        }
                        for c in app.update(m) {
                            let _ = self.execute(c, app).await;
                        }
                    }
                    let _ = crate::terminal::clear_frame(screen);
                    match result {
                        Ok(Some(text)) => app.chrome.flash(text, false),
                        Ok(None) => {}
                        Err(e) => app.chrome.flash(e, true),
                    }
                } else if self.execute(cmd, app).await? {
                    return Ok(());
                }
            }
            screen
                .draw(|f| app.render(f))
                .map_err(|e| format!("draw: {e}"))?;
        }
    }

    /// Executes one command. Returns `true` when the runtime should quit.
    async fn execute(&mut self, cmd: Cmd, app: &mut App) -> Result<bool, String> {
        match cmd {
            Cmd::Quit => Ok(true),
            Cmd::Connect { context } => {
                let tx = self.tx.clone();
                let sessions = self.sessions.clone();
                let config = self.config.clone();
                tokio::spawn(async move {
                    let result = connect_one(&sessions, &config, &context).await;
                    let _ = tx.send(Msg::Connected { context, result });
                });
                Ok(false)
            }
            Cmd::OpenKind { query } => {
                let cmds = app.exec_command(&query);
                for c in cmds {
                    if Box::pin(self.execute(c, app)).await? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Cmd::SaveResource {
                view,
                kind,
                ns,
                name,
            } => {
                let tx = self.tx.clone();
                let sessions = self.sessions.clone();
                tokio::spawn(async move {
                    let result = save_resource(&sessions, &kind, ns.as_deref(), &name).await;
                    let _ = view;
                    let _ = tx.send(match result {
                        Ok(path) => Msg::Flash {
                            text: format!("saved {path}"),
                            error: false,
                        },
                        Err(e) => Msg::Flash {
                            text: e,
                            error: true,
                        },
                    });
                });
                Ok(false)
            }
            Cmd::Ping { context } => {
                let tx = self.tx.clone();
                let sessions = self.sessions.clone();
                tokio::spawn(async move {
                    let result = ping_one(&sessions, &context).await;
                    let _ = tx.send(Msg::Pinged { context, result });
                });
                Ok(false)
            }
            Cmd::StartTableWatch {
                view,
                kind,
                ns,
                selector,
            } => {
                let client = {
                    let s = self.sessions.lock().expect("sessions lock");
                    s.active_session().map(|sess| sess.client.clone())
                };
                let Some(client) = client else {
                    let _ = self.tx.send(Msg::Error {
                        view: Some(view),
                        text: "not connected".into(),
                    });
                    return Ok(false);
                };
                let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    run_table_watch(
                        client,
                        kind.group.clone(),
                        kind.version.clone(),
                        kind.kind.clone(),
                        kind.plural.clone(),
                        ns,
                        selector,
                        move |ev: kxs_cluster::resources::TableEvent| {
                            tx.send(Msg::Table { view, ev }).is_ok()
                        },
                        stop_rx,
                    )
                    .await;
                });
                let _ = self.tx.send(Msg::Started {
                    view,
                    handle: StopHandle(stop_tx),
                });
                Ok(false)
            }
            Cmd::StartPodWatch { view, ns, selector } => {
                let client = {
                    let s = self.sessions.lock().expect("sessions lock");
                    s.active_session().map(|sess| sess.client.clone())
                };
                let Some(client) = client else {
                    let _ = self.tx.send(Msg::Error {
                        view: Some(view),
                        text: "not connected".into(),
                    });
                    return Ok(false);
                };
                let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    run_pod_watch(
                        client,
                        ns,
                        selector,
                        move |ev: kxs_cluster::pods::PodEvent| {
                            tx.send(Msg::Pod { view, ev }).is_ok()
                        },
                        stop_rx,
                    )
                    .await;
                });
                let _ = self.tx.send(Msg::Started {
                    view,
                    handle: StopHandle(stop_tx),
                });
                Ok(false)
            }
            Cmd::StartLogs { view, req } => {
                let client = {
                    let s = self.sessions.lock().expect("sessions lock");
                    s.active_session().map(|sess| sess.client.clone())
                };
                let Some(client) = client else {
                    let _ = self.tx.send(Msg::Error {
                        view: Some(view),
                        text: "not connected".into(),
                    });
                    return Ok(false);
                };
                let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
                let tx = self.tx.clone();
                let pod = req.pod.clone();
                tokio::spawn(async move {
                    run_log_stream(
                        client,
                        req,
                        move |ev: kxs_cluster::logs::LogEvent| match ev {
                            kxs_cluster::logs::LogEvent::Lines { lines } => tx
                                .send(Msg::LogLines {
                                    view,
                                    pod: pod.clone(),
                                    lines,
                                })
                                .is_ok(),
                            kxs_cluster::logs::LogEvent::Error { message } => tx
                                .send(Msg::LogStatus {
                                    view,
                                    pod: pod.clone(),
                                    status: Err(message),
                                })
                                .is_ok(),
                            kxs_cluster::logs::LogEvent::Eof => tx
                                .send(Msg::LogStatus {
                                    view,
                                    pod: pod.clone(),
                                    status: Ok(()),
                                })
                                .is_ok(),
                        },
                        stop_rx,
                    )
                    .await;
                });
                let _ = self.tx.send(Msg::Started {
                    view,
                    handle: StopHandle(stop_tx),
                });
                Ok(false)
            }
            Cmd::StartForward {
                view,
                ns,
                pod,
                port,
            } => {
                let client = {
                    let s = self.sessions.lock().expect("sessions lock");
                    s.active_session().map(|sess| sess.client.clone())
                };
                let Some(client) = client else {
                    let _ = self.tx.send(Msg::Error {
                        view: Some(view),
                        text: "not connected".into(),
                    });
                    return Ok(false);
                };
                let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
                let started =
                    kxs_cluster::portforward::start(client, ns.clone(), pod.clone(), port, stop_rx)
                        .await;
                match started {
                    Ok((local_port, _handle)) => {
                        let id = {
                            let mut s = self.sessions.lock().expect("sessions lock");
                            let id = s.next_forward_id();
                            s.add_forward(crate::sessions::Forward {
                                id,
                                ns: ns.clone(),
                                pod: pod.clone(),
                                container: None,
                                pod_port: port,
                                local_port,
                                started: std::time::Instant::now(),
                                stop: Some(stop_tx),
                            });
                            id
                        };
                        let _ = self.tx.send(Msg::ForwardStarted {
                            view,
                            id,
                            local_port,
                        });
                    }
                    Err(e) => {
                        let _ = self.tx.send(Msg::Error {
                            view: Some(view),
                            text: format!("port-forward: {e}"),
                        });
                    }
                }
                Ok(false)
            }
            Cmd::StopForward { id } => {
                self.sessions
                    .lock()
                    .expect("sessions lock")
                    .stop_forward(id);
                Ok(false)
            }
            Cmd::PickExec {
                view,
                ns,
                pod,
                options,
            } => {
                app.open_exec_pick(view, ns, pod, options);
                Ok(false)
            }
            Cmd::PreviewTheme { id } => {
                app.preview_theme(&id);
                Ok(false)
            }
            Cmd::PollMetrics { view, every } => {
                // one poller per session; abort the previous on :ctx switch
                if let Some(prev) = self.metrics_poller.take() {
                    prev.abort();
                }
                let client = {
                    let s = self.sessions.lock().expect("sessions lock");
                    s.active_session().map(|sess| sess.client.clone())
                };
                let Some(client) = client else {
                    return Ok(false);
                };
                let tx = self.tx.clone();
                self.metrics_poller = Some(tokio::spawn(async move {
                    let mut tick = tokio::time::interval(every);
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        let pods = kxs_cluster::metrics::pod_metrics(client.clone(), None).await;
                        let nodes = kxs_cluster::metrics::node_metrics(client.clone()).await;
                        if tx.send(Msg::Metrics { view, pods, nodes }).is_err() {
                            break;
                        }
                    }
                }));
                Ok(false)
            }
            Cmd::Suspend(_) => unreachable!("handled inline in run()"),
            Cmd::Fetch { view, what } => {
                let tx = self.tx.clone();
                let sessions = self.sessions.clone();
                tokio::spawn(async move {
                    let result = fetch(&sessions, what).await;
                    let _ = tx.send(Msg::Fetched { view, result });
                });
                Ok(false)
            }
            Cmd::Mutate { view, m } => {
                let client = {
                    let s = self.sessions.lock().expect("sessions lock");
                    s.active_session().map(|sess| sess.client.clone())
                };
                let Some(client) = client else {
                    let _ = self.tx.send(Msg::Error {
                        view: Some(view),
                        text: "not connected".into(),
                    });
                    return Ok(false);
                };
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = crate::suspend::mutate(client, m.clone()).await;
                    let _ = tx.send(Msg::Mutated { view, m, result });
                });
                Ok(false)
            }
            Cmd::ConfirmUndo {
                view,
                ns,
                name,
                revision,
            } => {
                app.open_confirm_undo(view, ns, name, revision);
                Ok(false)
            }
            Cmd::PickContainer {
                view,
                ns,
                pod,
                options,
            } => {
                app.open_container_pick(view, ns, pod, options);
                Ok(false)
            }
            Cmd::PopView => {
                app.pop_view();
                Ok(false)
            }
            Cmd::SwitchNamespace { ns } => {
                app.set_namespace(ns.clone());
                app.record_favorite(ns);
                let cfg = self.config.lock().expect("config lock").clone();
                if let Err(e) = config::write(&cfg) {
                    app.chrome.flash(format!("config: {e}"), true);
                }
                Ok(false)
            }
            Cmd::Stop(handle) => {
                handle.stop();
                Ok(false)
            }
            Cmd::SaveConfig => {
                let cfg = self.config.lock().expect("config lock").clone();
                if let Err(e) = config::write(&cfg) {
                    app.chrome.flash(format!("config: {e}"), true);
                }
                Ok(false)
            }
        }
    }
}
