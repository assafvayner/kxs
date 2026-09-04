use std::sync::{Arc, Mutex};

use clap::Parser;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use kxs_core::kubeconfig::paths::kubeconfig_paths;
use kxs_core::kubeconfig::store::KubeconfigStore;

use kxs_tui::app::App;
use kxs_tui::cmd::Cmd;
use kxs_tui::config;
use kxs_tui::msg::Msg;
use kxs_tui::runtime::{connect_one, Runtime};
use kxs_tui::sessions::{Sessions, Shared};
use kxs_tui::{terminal, theme, views};

/// kxs terminal UI
#[derive(Parser, Debug)]
#[command(name = "kxs", about = "kxs terminal UI")]
struct Cli {
    /// Context to connect at startup
    #[arg(long)]
    context: Option<String>,
    /// Namespace to open (overrides config and kubeconfig)
    #[arg(long)]
    namespace: Option<String>,
    /// Kubeconfig path(s); overrides KUBECONFIG and ~/.kube/config
    #[arg(long = "kubeconfig")]
    kubeconfig: Vec<std::path::PathBuf>,
    /// Theme id (see :theme)
    #[arg(long)]
    theme: Option<String>,
    /// Hide mutating actions and refuse mutations
    #[arg(long)]
    readonly: bool,
    /// Run one `:` command after connecting
    #[arg(long)]
    command: Option<String>,
}

fn load_store(paths: Vec<std::path::PathBuf>) -> KubeconfigStore {
    let (store, warnings) = KubeconfigStore::load_tolerant(paths);
    for w in &warnings {
        eprintln!("kxs: {w}");
    }
    store
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let cli = Cli::parse();
    let (cfg, cfg_warning) = config::load();
    if let Some(w) = cfg_warning {
        eprintln!("kxs: {w}");
    }
    let theme_id = cli
        .theme
        .clone()
        .or_else(|| {
            let c = &cfg;
            c.theme.clone()
        })
        .unwrap_or_else(|| theme::DEFAULT_ID.to_string());
    let paths = if cli.kubeconfig.is_empty() {
        kubeconfig_paths()
    } else {
        cli.kubeconfig.clone()
    };
    let sessions: Shared = Arc::new(Mutex::new(Sessions::new(load_store(paths))));
    let config = Arc::new(Mutex::new(cfg));
    let mut app = App::new(sessions.clone(), config.clone(), theme::get(&theme_id));
    app.set_readonly_override(cli.readonly);

    // Startup connect runs before raw mode so exec-auth plugins can prompt.
    let startup_context = cli.context.clone().or_else(|| {
        sessions
            .lock()
            .expect("sessions lock")
            .store
            .current_context()
    });
    let mut connected_at_startup = false;
    if let Some(ctx) = &startup_context {
        print!("kxs: connecting to {ctx}…\r\n");
        use std::io::Write;
        let _ = std::io::stdout().flush();
        match connect_one(&sessions, &config, ctx).await {
            Ok(_) => {
                if let Some(ns) = &cli.namespace {
                    let mut s = sessions.lock().expect("sessions lock");
                    if let Some(a) = &mut s.active {
                        a.namespace = Some(ns.clone());
                    }
                }
                connected_at_startup = true;
            }
            Err(e) => eprintln!("kxs: connect {ctx}: {e}"),
        }
    }

    let (tx, rx) = mpsc::unbounded_channel::<Msg>();

    // initial views and their pending commands (watch starts, context pings)
    let mut pre_cmds: Vec<Cmd> = if connected_at_startup {
        let mut cmds = match views::resources::open(&mut app, "pods", None) {
            Some(view) => app.replace_views(vec![view]),
            None => {
                let view = Box::new(views::contexts::ContextsView::new(&mut app));
                app.push_view(view)
            }
        };
        if let Some(cmd) = &cli.command {
            cmds.extend(app.exec_command(cmd));
        }
        cmds
    } else {
        let view = Box::new(views::contexts::ContextsView::new(&mut app));
        app.push_view(view)
    };

    // kubeconfig watcher: reload the store on changes
    {
        let watch_paths = sessions.lock().expect("sessions lock").store.paths();
        let tx2 = tx.clone();
        kxs_core::watch::spawn_watcher(watch_paths, move || {
            let _ = tx2.send(Msg::KubeconfigChanged);
        });
    }

    terminal::install_panic_hook();
    let _guard = terminal::RestoreGuard;
    terminal::enter().map_err(|e| format!("terminal: {e}"))?;
    if std::env::var_os("KXS_TUI_DEBUG_PANIC").is_some() {
        panic!("debug panic: terminal restore check");
    }

    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend).map_err(|e| format!("terminal: {e}"))?;

    let mut runtime = Runtime::new(tx, sessions.clone(), config.clone());
    runtime
        .run(&mut app, rx, &mut terminal, std::mem::take(&mut pre_cmds))
        .await
}
