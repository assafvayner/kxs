//! The event loop: consumes `Msg`s, runs `Cmd`s as tokio tasks, redraws.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{Event, EventStream};
use ratatui::Terminal;
use tokio::sync::mpsc;

use kxs_cluster::bridge;
use kxs_cluster::discovery;
use kxs_cluster::logs::run_log_stream;
use kxs_cluster::pods::run_pod_watch;
use kxs_cluster::resources::{get_yaml, run_table_watch};
use kxs_cluster::session;

use crate::app::App;
use crate::cmd::{Cmd, Fetch, FetchResult, StopHandle};
use crate::config::{self, Config};
use crate::msg::Msg;
use crate::sessions::Shared;

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
    }
}

pub struct Runtime {
    tx: mpsc::UnboundedSender<Msg>,
    sessions: Shared,
    config: Arc<Mutex<Config>>,
    metrics_poller: Option<tokio::task::JoinHandle<()>>,
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
        }
    }

    /// Pump terminal events into the message channel.
    fn spawn_event_pump(&self) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let mut reader = EventStream::new();
            while let Some(Ok(ev)) = reader.next().await {
                let msg = match ev {
                    Event::Key(k) => Msg::Key(k),
                    Event::Resize(w, h) => Msg::Resize(w, h),
                    _ => continue,
                };
                if tx.send(msg).is_err() {
                    break;
                }
            }
        });
    }

    /// Runs until `Cmd::Quit` or the message channel closes. The terminal is
    /// owned here so nothing else renders.
    pub async fn run(
        &mut self,
        app: &mut App,
        mut rx: mpsc::UnboundedReceiver<Msg>,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
        pre_cmds: Vec<Cmd>,
    ) -> Result<(), String> {
        self.spawn_event_pump();
        for cmd in pre_cmds {
            if self.execute(cmd, app).await? {
                return Ok(());
            }
        }
        terminal
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
                if self.execute(cmd, app).await? {
                    return Ok(());
                }
            }
            terminal
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
            Cmd::Fetch { view, what } => {
                let tx = self.tx.clone();
                let sessions = self.sessions.clone();
                tokio::spawn(async move {
                    let result = fetch(&sessions, what).await;
                    let _ = tx.send(Msg::Fetched { view, result });
                });
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
