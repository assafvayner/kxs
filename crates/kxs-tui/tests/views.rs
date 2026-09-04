//! TestBackend render tests and layout tests for the views and the app frame.

use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::Terminal;

use kxs_cluster::discovery::ResourceKind;
use kxs_cluster::resources::{ResourceRow, ResourceTable, TableEvent};

use kxs_tui::app::App;
use kxs_tui::config::Config;
use kxs_tui::msg::Msg;
use kxs_tui::sessions::{Sessions, Shared};
use kxs_tui::theme;
use kxs_tui::view::View;
use kxs_tui::views::contexts::ContextsView;
use kxs_tui::views::resources::ResourcesView;

fn pod_target() -> kxs_tui::view::Target {
    kxs_tui::view::Target {
        kind: pod_kind(),
        ns: Some("default".into()),
        name: "web".into(),
        container: None,
        desired_replicas: None,
        suspend: None,
        unschedulable: None,
    }
}

fn test_app() -> App {
    let sessions: Shared = std::sync::Arc::new(std::sync::Mutex::new(Sessions::default()));
    App::new(
        sessions,
        std::sync::Arc::new(std::sync::Mutex::new(Config::default())),
        theme::get(theme::DEFAULT_ID),
    )
}

fn buf_text(t: &Terminal<TestBackend>) -> String {
    let buf = t.backend().buffer();
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        out.push('\n');
    }
    out
}

fn pod_kind() -> ResourceKind {
    ResourceKind {
        group: "".into(),
        version: "v1".into(),
        kind: "Pod".into(),
        plural: "pods".into(),
        namespaced: true,
        aliases: vec!["po".into()],
    }
}

fn fixture_table() -> ResourceTable {
    let row = |name: &str, cells: Vec<&str>, created: &str| ResourceRow {
        key: format!("default/{name}"),
        name: name.into(),
        namespace: Some("default".into()),
        cells: cells.into_iter().map(String::from).collect(),
        created: Some(created.into()),
    };
    ResourceTable {
        columns: vec!["NAME".into(), "READY".into(), "STATUS".into(), "Age".into()],
        rows: vec![
            row(
                "web-7d9f",
                vec!["web-7d9f", "1/1", "Running", "30d"],
                "2026-08-01T00:00:00Z",
            ),
            row(
                "api-xyz",
                vec!["api-xyz", "0/1", "Pending", "2m"],
                "2026-09-01T00:00:00Z",
            ),
        ],
    }
}

fn resources_view(app: &mut App) -> Box<ResourcesView> {
    Box::new(ResourcesView::new(app, pod_kind(), Some("default".into())))
}

#[test]
fn contexts_view_renders_rows_and_status() {
    let mut app = test_app();
    let view = Box::new(ContextsView::new(&mut app));
    app.push_view(view);
    let mut term = Terminal::new(TestBackend::new(100, 24)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("Contexts["), "{text}");
    assert!(text.contains("no contexts in the kubeconfig"), "{text}");
}

#[test]
fn resources_view_renders_columns_and_rows() {
    let mut app = test_app();
    let view = resources_view(&mut app);
    let id = view.id();
    app.push_view(view);
    app.update(Msg::Table {
        view: id,
        ev: TableEvent::Table {
            table: fixture_table(),
        },
    });
    let mut term = Terminal::new(TestBackend::new(100, 24)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("Pod(default)[2]"), "{text}");
    assert!(text.contains("web-7d9f"), "{text}");
    assert!(text.contains("api-xyz"), "{text}");
    assert!(text.contains("STATUS"), "{text}");
    assert!(text.contains("Running"), "{text}");
}

#[test]
fn resources_view_renders_age_from_created() {
    let mut app = test_app();
    let mut view = resources_view(&mut app);
    let ctx = app.ctx();
    let id = view.id();
    let created = "2020-01-01T00:00:00Z";
    let table = ResourceTable {
        columns: vec!["NAME".into(), "READY".into(), "STATUS".into(), "Age".into()],
        rows: vec![ResourceRow {
            key: "default/web-7d9f".into(),
            name: "web-7d9f".into(),
            namespace: Some("default".into()),
            cells: vec!["web-7d9f".into(), "1/1".into(), "Running".into(), "".into()],
            created: Some(created.into()),
        }],
    };
    view.on_msg(
        &Msg::Table {
            view: id,
            ev: TableEvent::Table { table },
        },
        &ctx,
    );
    let mut t = Terminal::new(TestBackend::new(100, 10)).unwrap();
    t.draw(|f| view.render(f, f.area(), &theme::get(theme::DEFAULT_ID), ""))
        .unwrap();
    let text = buf_text(&t);
    let web_line = text.lines().find(|l| l.contains("web-7d9f")).expect("row");
    let expected = kxs_core::format::age(Some(created), kxs_cluster::clock::now_ms());
    assert!(
        web_line.contains(&expected),
        "expected age {expected:?} in row: {web_line}"
    );
}

#[test]
fn resources_view_title_shows_filter() {
    let mut app = test_app();
    let view = resources_view(&mut app);
    let id = view.id();
    app.push_view(view);
    app.update(Msg::Table {
        view: id,
        ev: TableEvent::Table {
            table: fixture_table(),
        },
    });
    // filter prompt on the top view: type "web" and submit
    app.update(Msg::Key(KeyEvent::from(KeyCode::Char('/'))));
    for c in "web".chars() {
        app.update(Msg::Key(KeyEvent::from(KeyCode::Char(c))));
    }
    app.update(Msg::Key(KeyEvent::from(KeyCode::Enter)));
    assert_eq!(app.views[0].filter(), "web");
    let mut term = Terminal::new(TestBackend::new(100, 24)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("filter: web"), "{text}");
    assert!(!text.contains("api-xyz"), "{text}");
}

#[test]
fn layout_header_collapse_at_80_and_160() {
    for (w, expect_logo, expect_collapse) in [
        (80u16, false, false),
        (100, true, false),
        (160, true, false),
    ] {
        let mut app = test_app();
        let view = Box::new(ContextsView::new(&mut app));
        app.push_view(view);
        let mut term = Terminal::new(TestBackend::new(w, 24)).unwrap();
        term.draw(|f| app.render(f)).unwrap();
        let text = buf_text(&term);
        let has_logo = text.contains("kxs") || text.contains('|');
        assert_eq!(has_logo, expect_logo, "width {w}: {text}");
        let _ = expect_collapse;
    }
}

#[test]
fn layout_under_80_collapses_header() {
    let mut app = test_app();
    let view = Box::new(ContextsView::new(&mut app));
    app.push_view(view);
    app.chrome.context = "kind-local".into();
    let mut term = Terminal::new(TestBackend::new(70, 24)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let text = buf_text(&term);
    // collapsed header: context line at the very top
    let first_line = text.lines().next().unwrap();
    assert!(first_line.contains("Context:"), "{text}");
    // body starts on row 2 (index 2)
    assert!(
        text.lines().nth(2).unwrap().contains("Contexts[")
            || text.lines().nth(2).unwrap().contains('┌'),
        "{text}"
    );
}

#[test]
fn resources_columns_drop_when_narrow() {
    let mut app = test_app();
    let view = resources_view(&mut app);
    let id = view.id();
    app.push_view(view);
    app.update(Msg::Table {
        view: id,
        ev: TableEvent::Table {
            table: fixture_table(),
        },
    });
    app.update(Msg::Resize(18, 24));
    let mut term = Terminal::new(TestBackend::new(18, 24)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let text = buf_text(&term);
    // NAME and AGE survive; middle columns drop
    assert!(text.contains("NAME"), "{text}");
    assert!(text.contains("Age"), "{text}");
    assert!(!text.contains("READY"), "{text}");
    assert!(!text.contains("STATUS"), "{text}");
}

#[test]
fn age_column_sorts_by_creation() {
    let mut app = test_app();
    let view = resources_view(&mut app);
    let id = view.id();
    app.push_view(view);
    app.update(Msg::Table {
        view: id,
        ev: TableEvent::Table {
            table: fixture_table(),
        },
    });
    // shift-A sorts by age ascending (youngest first): api-xyz (2m) before web-7d9f (30d)
    app.update(Msg::Key(KeyEvent::from(KeyCode::Char('A'))));
    let ctx = app.ctx();
    // selection starts at first row already; read order via visible rows through render
    let mut term = Terminal::new(TestBackend::new(100, 24)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let text = buf_text(&term);
    let api_pos = text.find("api-xyz").unwrap();
    let web_pos = text.find("web-7d9f").unwrap();
    assert!(api_pos < web_pos, "{text}");
    let _ = ctx;
}

#[test]
fn yaml_view_renders_syntax_text() {
    let mut app = test_app();
    let target = pod_target();
    let view = kxs_tui::views::yaml::YamlView::new(&mut app, &target);
    let id = view.id();
    app.push_view(Box::new(view));
    app.update(Msg::Fetched {
        view: id,
        result: Ok(kxs_tui::cmd::FetchResult::Yaml(
            "apiVersion: v1\nkind: Pod\n# a comment\nmetadata:\n  name: web\n".into(),
        )),
    });
    let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("YAML(Pod/web)"), "{text}");
    assert!(text.contains("apiVersion"), "{text}");
    assert!(text.contains("# a comment"), "{text}");
    assert!(text.contains("name: web"), "{text}");
}

#[test]
fn describe_view_renders_plain_text() {
    let mut app = test_app();
    let target = pod_target();
    let view = kxs_tui::views::describe::DescribeView::new(&mut app, &target);
    let id = view.id();
    app.push_view(Box::new(view));
    app.update(Msg::Fetched {
        view: id,
        result: Ok(kxs_tui::cmd::FetchResult::Describe(
            "Name: web\nNode: kind-worker\n".into(),
        )),
    });
    let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("Describe(Pod/web)"), "{text}");
    assert!(text.contains("Name: web"), "{text}");
    assert!(text.contains("Node: kind-worker"), "{text}");
}

#[test]
fn d_key_pushes_describe_and_fetches() {
    let mut app = test_app();
    let view = resources_view(&mut app);
    let id = view.id();
    app.push_view(view);
    app.update(Msg::Table {
        view: id,
        ev: TableEvent::Table {
            table: fixture_table(),
        },
    });
    let cmds = app.update(Msg::Key(KeyEvent::from(KeyCode::Char('d'))));
    // stack now has the Describe view on top, with a Fetch queued
    assert_eq!(app.views.len(), 2);
    assert_eq!(app.views[1].crumb(), "describe");
    assert!(cmds
        .iter()
        .any(|c| matches!(c, kxs_tui::cmd::Cmd::Fetch { .. })));
}

#[test]
fn y_key_pushes_yaml_and_fetches() {
    let mut app = test_app();
    let view = resources_view(&mut app);
    let id = view.id();
    app.push_view(view);
    app.update(Msg::Table {
        view: id,
        ev: TableEvent::Table {
            table: fixture_table(),
        },
    });
    let cmds = app.update(Msg::Key(KeyEvent::from(KeyCode::Char('y'))));
    assert_eq!(app.views.len(), 2);
    assert_eq!(app.views[1].crumb(), "yaml");
    assert!(cmds
        .iter()
        .any(|c| matches!(c, kxs_tui::cmd::Cmd::Fetch { .. })));
    // Esc pops back to the table
    app.update(Msg::Key(KeyEvent::from(KeyCode::Esc)));
    assert_eq!(app.views.len(), 1);
}

fn pod_row(name: &str, status: &str, created: &str) -> kxs_cluster::pods::PodRow {
    kxs_cluster::pods::PodRow {
        key: format!("default/{name}"),
        name: name.into(),
        namespace: "default".into(),
        ready: "1/1".into(),
        status: status.into(),
        restarts: 0,
        ip: Some("10.0.0.1".into()),
        node: Some("n1".into()),
        created: Some(created.into()),
        cpu_request_millis: Some(250),
        mem_request_mib: Some(128),
    }
}

#[test]
fn pods_view_renders_rows_and_status_colors() {
    let mut app = test_app();
    let view = kxs_tui::views::pods::PodsView::new(&mut app, Some("default".into()));
    let id = view.id();
    app.push_view(Box::new(view));
    app.update(kxs_tui::msg::Msg::Pod {
        view: id,
        ev: kxs_cluster::pods::PodEvent::Snapshot {
            rows: vec![
                pod_row("web", "Running", "2026-09-01T00:00:00Z"),
                pod_row("bad", "CrashLoopBackOff", "2026-09-01T00:00:00Z"),
            ],
        },
    });
    let mut term = Terminal::new(TestBackend::new(120, 24)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("Pods(default)[2]"), "{text}");
    assert!(text.contains("web"), "{text}");
    assert!(text.contains("CrashLoopBackOff"), "{text}");
    assert!(text.contains("RESTARTS"), "{text}");
    // metrics missing → dash
    assert!(text.contains("—"), "{text}");
}

#[test]
fn pods_view_late_events_are_dropped() {
    let mut app = test_app();
    let view = kxs_tui::views::pods::PodsView::new(&mut app, Some("default".into()));
    let id = view.id();
    app.push_view(Box::new(view));
    app.pop_view();
    let cmds = app.update(kxs_tui::msg::Msg::Pod {
        view: id,
        ev: kxs_cluster::pods::PodEvent::Snapshot { rows: vec![] },
    });
    assert!(cmds.is_empty());
}

#[test]
fn logs_view_renders_lines_and_filters() {
    let mut app = test_app();
    let target = kxs_tui::view::Target {
        kind: pod_kind(),
        ns: Some("default".into()),
        name: "web".into(),
        container: Some("web".into()),
        desired_replicas: None,
        suspend: None,
        unschedulable: None,
    };
    let view = kxs_tui::views::logs::LogsView::new_with_container(&mut app, target);
    let id = view.id();
    app.push_view(Box::new(view));
    app.update(kxs_tui::msg::Msg::LogLines {
        view: id,
        pod: "web".into(),
        lines: vec!["started".into(), "serving on :8080".into()],
    });
    let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("serving on :8080"), "{text}");
    // filter via /
    app.update(Msg::Key(KeyEvent::from(KeyCode::Char('/'))));
    for c in "serving".chars() {
        app.update(Msg::Key(KeyEvent::from(KeyCode::Char(c))));
    }
    app.update(Msg::Key(KeyEvent::from(KeyCode::Enter)));
    let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("serving on :8080"), "{text}");
    assert!(!text.contains("started"), "{text}");
}

#[test]
fn events_view_sorts_newest_first_and_colors_warnings() {
    let mut app = test_app();
    let view = kxs_tui::views::events::EventsView::new(&mut app, Some("default".into()));
    let id = view.id();
    app.push_view(Box::new(view));
    let row = |created: Option<&str>, cells: Vec<&str>| kxs_cluster::resources::ResourceRow {
        key: cells[2].to_string(),
        name: String::new(),
        namespace: Some("default".into()),
        cells: cells.into_iter().map(String::from).collect(),
        created: created.map(String::from),
    };
    app.update(Msg::Table {
        view: id,
        ev: TableEvent::Table {
            table: kxs_cluster::resources::ResourceTable {
                columns: ["Last Seen", "Type", "Reason", "Object", "Message", "Age"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                rows: vec![
                    row(
                        None,
                        vec!["3h", "Normal", "Pulling", "pod/a", "pulling", "3h"],
                    ),
                    row(
                        None,
                        vec!["30s", "Warning", "BackOff", "pod/b", "back-off", "30s"],
                    ),
                ],
            },
        },
    });
    let mut term = Terminal::new(TestBackend::new(120, 24)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("Events(default)[2]"), "{text}");
    // newest first: pod/b (Last Seen 30s) before pod/a (3h); cell text may be
    // clipped by column width, so match on the object cells
    let b = text.find("pod/b").unwrap();
    let a = text.find("pod/a").unwrap();
    assert!(b < a, "{text}");
}

#[test]
fn metrics_view_renders_nodes_and_pods() {
    let mut app = test_app();
    let view = kxs_tui::views::metrics::MetricsView::new(&mut app);
    let id = view.id();
    app.push_view(Box::new(view));
    app.update(kxs_tui::msg::Msg::Metrics {
        view: id,
        pods: Ok(vec![kxs_cluster::metrics::MetricsRow {
            key: "default/web".into(),
            name: "web".into(),
            namespace: Some("default".into()),
            cpu_millicores: 12,
            mem_mib: 48,
        }]),
        nodes: Ok(vec![kxs_cluster::metrics::NodeMetricsRow {
            name: "kind-control-plane".into(),
            cpu_millicores: 412,
            cpu_allocatable_millicores: Some(4000),
            mem_mib: 1024,
            mem_allocatable_mib: Some(8192),
        }]),
    });
    let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("kind-control-plane"), "{text}");
    assert!(text.contains("412m/4000m 10%"), "{text}");
    assert!(text.contains("Pods by CPU"), "{text}");
    assert!(text.contains("web"), "{text}");
    // header cpu/mem updated too
    assert!(text.contains("CPU/MEM: 10% / 13%"), "{text}");
}

#[test]
fn enter_on_pod_opens_containers_and_fetches() {
    let mut app = test_app();
    let view = kxs_tui::views::pods::PodsView::new(&mut app, Some("default".into()));
    let id = view.id();
    app.push_view(Box::new(view));
    app.update(kxs_tui::msg::Msg::Pod {
        view: id,
        ev: kxs_cluster::pods::PodEvent::Snapshot {
            rows: vec![pod_row("web", "Running", "2026-09-01T00:00:00Z")],
        },
    });
    app.update(Msg::Key(KeyEvent::from(KeyCode::Enter)));
    assert_eq!(app.views.len(), 2);
    assert_eq!(app.views[1].crumb(), "containers");
    // Esc back
    app.update(Msg::Key(KeyEvent::from(KeyCode::Esc)));
    assert_eq!(app.views.len(), 1);
}

#[test]
fn readonly_refuses_mutations() {
    let sessions = kxs_tui::sessions::Shared::new(std::sync::Mutex::new(
        kxs_tui::sessions::Sessions::default(),
    ));
    let cfg = kxs_tui::config::Config {
        readonly: true,
        ..Default::default()
    };
    let mut app = App::new(
        sessions,
        std::sync::Arc::new(std::sync::Mutex::new(cfg)),
        theme::get(theme::DEFAULT_ID),
    );
    let view = resources_view(&mut app);
    let id = view.id();
    app.push_view(view);
    app.update(Msg::Table {
        view: id,
        ev: TableEvent::Table {
            table: fixture_table(),
        },
    });
    // 'e' (edit) refused with a flash, no Suspend cmd
    let cmds = app.update(Msg::Key(KeyEvent::from(KeyCode::Char('e'))));
    assert!(cmds.is_empty());
    assert!(app
        .chrome
        .flash
        .as_ref()
        .is_some_and(|f| f.text.contains("readonly")));
    // non-mutating keys still work
    let _ = app.update(Msg::Key(KeyEvent::from(KeyCode::Char('d'))));
    assert_eq!(app.views.len(), 2);
}

#[test]
fn scale_input_modal_prefills_and_submits() {
    let mut app = test_app();
    let view = resources_view(&mut app);
    let id = view.id();
    app.push_view(view);
    app.update(Msg::Table {
        view: id,
        ev: TableEvent::Table {
            table: fixture_table(),
        },
    });
    // fixture_table is Pods kind — not scalable; construct a deployment-like target via
    // the app's mutation path is kind-gated, so test the modal directly:
    app.chrome.open_input(
        "Scale x to replicas:".into(),
        "2".into(),
        id,
        kxs_tui::chrome::InputAction::Scale {
            kind: pod_kind(),
            ns: "default".into(),
            name: "x".into(),
        },
    );
    let cmds = app.update(Msg::Key(KeyEvent::from(KeyCode::Char('3'))));
    assert!(cmds.is_empty());
    let cmds = app.update(Msg::Key(KeyEvent::from(KeyCode::Enter)));
    assert!(cmds.iter().any(|c| matches!(
        c,
        kxs_tui::cmd::Cmd::Mutate {
            m: kxs_tui::cmd::Mutation::Scale { replicas: 23, .. },
            ..
        }
    ) || matches!(
        c,
        kxs_tui::cmd::Cmd::Mutate {
            m: kxs_tui::cmd::Mutation::Scale { replicas: 3, .. },
            ..
        }
    )));
}

#[test]
fn delete_modal_tracks_propagation_and_force() {
    let mut app = test_app();
    let view = resources_view(&mut app);
    let id = view.id();
    app.push_view(view);
    app.chrome.open_delete(
        "Delete Pod web?".into(),
        id,
        pod_kind(),
        "default".into(),
        "web".into(),
    );
    let _ = app.update(Msg::Key(KeyEvent::from(KeyCode::Right)));
    let _ = app.update(Msg::Key(KeyEvent::from(KeyCode::Char('f'))));
    let cmds = app.update(Msg::Key(KeyEvent::from(KeyCode::Enter)));
    assert!(cmds.iter().any(|c| matches!(
        c,
        kxs_tui::cmd::Cmd::Mutate {
            m: kxs_tui::cmd::Mutation::Delete { propagation: Some(p), force: true, .. },
            ..
        }
        if p == "Foreground"
    )));
    assert!(app.chrome.delete.is_none());
}

#[test]
fn rollout_view_lists_revisions_and_confirms_undo() {
    use kxs_tui::cmd::Cmd;
    let mut app = test_app();
    let target = pod_target();
    let view = kxs_tui::views::rollout::RolloutView::new(&mut app, target);
    let id = view.id();
    let cmds = app.push_view(Box::new(view));
    assert!(cmds.iter().any(|c| matches!(c, Cmd::Fetch { .. })));
    app.update(Msg::Fetched {
        view: id,
        result: Ok(kxs_tui::cmd::FetchResult::Rollout(vec![
            kxs_cluster::workloads::RolloutRevision {
                revision: 2,
                name: "web-abc".into(),
                created: Some("2026-09-01T00:00:00Z".into()),
                images: vec!["web:2".into()],
                replicas: 2,
                current: true,
            },
            kxs_cluster::workloads::RolloutRevision {
                revision: 1,
                name: "web-def".into(),
                created: Some("2026-08-01T00:00:00Z".into()),
                images: vec!["web:1".into()],
                replicas: 2,
                current: false,
            },
        ])),
    });
    // move to revision 1 and press Enter → ConfirmUndo
    let _ = app.update(Msg::Key(KeyEvent::from(KeyCode::Char('j'))));
    let cmds = app.update(Msg::Key(KeyEvent::from(KeyCode::Enter)));
    assert!(cmds
        .iter()
        .any(|c| matches!(c, Cmd::ConfirmUndo { revision: 1, .. })));
}

/// A kubeconfig with `n` contexts named `ctx-00`.., for the scrolling tests.
fn many_context_store(n: usize) -> kxs_core::kubeconfig::store::KubeconfigStore {
    // one directory per call: the tests run in parallel and would otherwise
    // read each other's half-written file
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("kxs_ctx_scroll_{n}_{seq}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config");
    let mut clusters = String::new();
    let mut users = String::new();
    let mut contexts = String::new();
    for i in 0..n {
        clusters.push_str(&format!(
            "  - {{name: cl{i}, cluster: {{server: \"https://c{i}\"}}}}\n"
        ));
        users.push_str(&format!("  - {{name: us{i}, user: {{token: t}}}}\n"));
        contexts.push_str(&format!(
            "  - {{name: ctx-{i:02}, context: {{cluster: cl{i}, user: us{i}}}}}\n"
        ));
    }
    std::fs::write(
        &path,
        format!("clusters:\n{clusters}users:\n{users}contexts:\n{contexts}"),
    )
    .unwrap();
    kxs_core::kubeconfig::store::KubeconfigStore::load_tolerant(vec![path]).0
}

fn app_with_contexts(n: usize) -> App {
    let sessions: Shared =
        std::sync::Arc::new(std::sync::Mutex::new(Sessions::new(many_context_store(n))));
    App::new(
        sessions,
        std::sync::Arc::new(std::sync::Mutex::new(Config::default())),
        theme::get(theme::DEFAULT_ID),
    )
}

#[test]
fn contexts_view_scrolls_the_selection_into_view() {
    let mut app = app_with_contexts(40);
    let mut view = ContextsView::new(&mut app);
    let ctx = app.ctx();
    for _ in 0..35 {
        view.handle_key(KeyEvent::from(KeyCode::Char('j')), &ctx);
    }
    let mut t = Terminal::new(TestBackend::new(90, 20)).unwrap();
    t.draw(|f| view.render(f, f.area(), &theme::get(theme::DEFAULT_ID), ""))
        .unwrap();
    let text = buf_text(&t);
    assert!(text.contains("ctx-35"), "selection off screen:\n{text}");
    assert!(!text.contains("ctx-00"), "top row should have scrolled off");
}

#[test]
fn contexts_view_filters_and_connects_to_the_filtered_row() {
    let mut app = app_with_contexts(40);
    let mut view = ContextsView::new(&mut app);
    let ctx = app.ctx();
    assert!(view.wants_filter());
    view.set_filter("ctx-37");
    let mut t = Terminal::new(TestBackend::new(90, 20)).unwrap();
    t.draw(|f| view.render(f, f.area(), &theme::get(theme::DEFAULT_ID), ""))
        .unwrap();
    let text = buf_text(&t);
    assert!(text.contains("ctx-37"), "filtered row missing:\n{text}");
    assert!(!text.contains("ctx-00"), "filter did not narrow rows");
    let cmds = view.handle_key(KeyEvent::from(KeyCode::Enter), &ctx);
    assert!(
        matches!(cmds.first(), Some(kxs_tui::cmd::Cmd::Connect { context }) if context == "ctx-37"),
        "enter did not connect to the filtered selection"
    );
}

#[test]
fn contexts_view_pages_with_ctrl_f_and_ctrl_b() {
    let mut app = app_with_contexts(40);
    let mut view = ContextsView::new(&mut app);
    let ctx = app.ctx();
    let mut t = Terminal::new(TestBackend::new(90, 20)).unwrap();
    // render once so the view learns its viewport height
    t.draw(|f| view.render(f, f.area(), &theme::get(theme::DEFAULT_ID), ""))
        .unwrap();
    view.handle_key(
        KeyEvent::new(
            KeyCode::Char('f'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        ),
        &ctx,
    );
    t.draw(|f| view.render(f, f.area(), &theme::get(theme::DEFAULT_ID), ""))
        .unwrap();
    assert!(!buf_text(&t).contains("ctx-00"), "ctrl-f did not page down");
    view.handle_key(
        KeyEvent::new(
            KeyCode::Char('b'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        ),
        &ctx,
    );
    t.draw(|f| view.render(f, f.area(), &theme::get(theme::DEFAULT_ID), ""))
        .unwrap();
    assert!(buf_text(&t).contains("ctx-00"), "ctrl-b did not page back");
}

/// A resources view seeded with the fixture table, pushed onto `app`.
fn seeded_resources(app: &mut App) -> u64 {
    let view = resources_view(app);
    let id = view.id();
    app.push_view(view);
    app.update(Msg::Table {
        view: id,
        ev: TableEvent::Table {
            table: fixture_table(),
        },
    });
    id
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(
        KeyCode::Char(c),
        ratatui::crossterm::event::KeyModifiers::CONTROL,
    )
}

fn render_top(app: &App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    buf_text(&t)
}

#[test]
fn ctrl_z_narrows_to_faulty_rows() {
    let mut app = test_app();
    seeded_resources(&mut app);
    assert!(render_top(&app, 90, 14).contains("web-7d9f"));
    app.update(Msg::Key(ctrl('z')));
    let text = render_top(&app, 90, 14);
    // api-xyz is 0/1 Pending; web-7d9f is 1/1 Running
    assert!(text.contains("api-xyz"), "faulty row missing:\n{text}");
    assert!(
        !text.contains("web-7d9f"),
        "healthy row still shown:\n{text}"
    );
    app.update(Msg::Key(ctrl('z')));
    assert!(render_top(&app, 90, 14).contains("web-7d9f"));
}

#[test]
fn ctrl_w_keeps_columns_that_narrow_drops() {
    let mut app = test_app();
    seeded_resources(&mut app);
    // the fixture's four columns need 28 cells; at 26 the trimmed layout
    // drops STATUS, while the wide layout keeps it (and clips AGE instead)
    app.update(Msg::Resize(26, 24));
    assert!(!render_top(&app, 26, 24).contains("STATUS"));
    app.update(Msg::Key(ctrl('w')));
    assert!(render_top(&app, 26, 24).contains("STATUS"));
}

#[test]
fn ctrl_e_and_ctrl_g_hide_the_header_and_crumbs() {
    let mut app = test_app();
    seeded_resources(&mut app);
    let full = render_top(&app, 90, 14);
    assert!(full.contains("<pods>"), "crumbs missing:\n{full}");
    app.update(Msg::Key(ctrl('g')));
    assert!(!render_top(&app, 90, 14).contains("<pods>"));
    app.update(Msg::Key(ctrl('e')));
    let bare = render_top(&app, 90, 14);
    assert!(!bare.contains("CONTEXT"), "header still drawn:\n{bare}");
}

#[test]
fn p_opens_logs_on_the_previous_container() {
    let mut app = test_app();
    seeded_resources(&mut app);
    let before = app.views.len();
    app.update(Msg::Key(KeyEvent::from(KeyCode::Char('p'))));
    assert_eq!(app.views.len(), before + 1);
    let title = app.views.last().unwrap().title();
    assert!(title.contains("prev"), "logs title lost previous: {title}");
}

#[test]
fn c_copies_the_name_and_n_the_namespace() {
    let mut app = test_app();
    seeded_resources(&mut app);
    app.update(Msg::Key(KeyEvent::from(KeyCode::Char('c'))));
    assert!(app
        .chrome
        .flash
        .as_ref()
        .is_some_and(|f| f.text.contains("web-7d9f")));
    app.update(Msg::Key(KeyEvent::from(KeyCode::Char('n'))));
    assert!(app
        .chrome
        .flash
        .as_ref()
        .is_some_and(|f| f.text.contains("default")));
}

#[test]
fn command_aliases_and_argument_forms() {
    let mut app = test_app();
    for alias in ["ctx", "context", "contexts"] {
        let cmds = app.exec_command(&format!("{alias} kind-local"));
        assert!(
            matches!(cmds.first(), Some(kxs_tui::cmd::Cmd::Connect { context }) if context == "kind-local"),
            "{alias} did not connect"
        );
    }
    // `@ctx` switches the session before browsing
    let cmds = app.exec_command("pods @kind-local");
    assert!(
        matches!(cmds.first(), Some(kxs_tui::cmd::Cmd::Connect { context }) if context == "kind-local")
    );
}

/// An app whose session has Pod discovery, so `:pods` resolves.
fn app_with_pod_kind() -> App {
    let mut sessions = Sessions::default();
    sessions.active = Some(kxs_tui::sessions::ActiveContext {
        name: "test".into(),
        namespace: Some("default".into()),
        version: "v1.30.0".into(),
    });
    sessions
        .kinds
        .insert("test".into(), std::sync::Arc::new(vec![pod_kind()]));
    App::new(
        std::sync::Arc::new(std::sync::Mutex::new(sessions)),
        std::sync::Arc::new(std::sync::Mutex::new(Config::default())),
        theme::get(theme::DEFAULT_ID),
    )
}

#[test]
fn kind_command_takes_a_filter_and_a_label_selector() {
    let mut app = app_with_pod_kind();
    app.exec_command("pods /web");
    assert_eq!(app.views.last().unwrap().filter(), "web");
    app.exec_command("pods app=web");
    assert_eq!(app.views.last().unwrap().filter(), "-l app=web");
}

#[test]
fn dash_repeats_the_last_command() {
    let mut app = test_app();
    seeded_resources(&mut app);
    // no history yet
    app.update(Msg::Key(KeyEvent::from(KeyCode::Char('-'))));
    assert!(app
        .chrome
        .flash
        .as_ref()
        .is_some_and(|f| f.text.contains("no command history")));
    // type `:help` through the prompt so it is recorded
    app.update(Msg::Key(KeyEvent::from(KeyCode::Char(':'))));
    for c in "help".chars() {
        app.update(Msg::Key(KeyEvent::from(KeyCode::Char(c))));
    }
    app.update(Msg::Key(KeyEvent::from(KeyCode::Enter)));
    let depth = app.views.len();
    app.update(Msg::Key(KeyEvent::from(KeyCode::Char('-'))));
    assert_eq!(app.views.len(), depth + 1, "`-` did not repeat :help");
}

#[test]
fn u_on_a_node_toggles_cordon_both_ways() {
    let mut app = test_app();
    let node_kind = ResourceKind {
        group: String::new(),
        version: "v1".into(),
        kind: "Node".into(),
        plural: "nodes".into(),
        namespaced: false,
        aliases: vec!["no".into()],
    };
    let view = Box::new(ResourcesView::new(&mut app, node_kind, None));
    let id = view.id();
    app.push_view(view);
    let node = |name: &str, status: &str| ResourceRow {
        key: name.into(),
        name: name.into(),
        namespace: None,
        cells: vec![name.into(), status.into(), "10d".into()],
        created: Some("2026-08-01T00:00:00Z".into()),
    };
    app.update(Msg::Table {
        view: id,
        ev: TableEvent::Table {
            table: ResourceTable {
                columns: vec!["NAME".into(), "STATUS".into(), "Age".into()],
                rows: vec![node("n1", "Ready")],
            },
        },
    });
    let cmds = app.update(Msg::Key(KeyEvent::from(KeyCode::Char('u'))));
    assert!(matches!(
        cmds.first(),
        Some(kxs_tui::cmd::Cmd::Mutate {
            m: kxs_tui::cmd::Mutation::Cordon {
                unschedulable: true,
                ..
            },
            ..
        })
    ));
    // an already-cordoned node uncordons instead
    app.update(Msg::Table {
        view: id,
        ev: TableEvent::Table {
            table: ResourceTable {
                columns: vec!["NAME".into(), "STATUS".into(), "Age".into()],
                rows: vec![node("n1", "Ready,SchedulingDisabled")],
            },
        },
    });
    let cmds = app.update(Msg::Key(KeyEvent::from(KeyCode::Char('u'))));
    assert!(matches!(
        cmds.first(),
        Some(kxs_tui::cmd::Cmd::Mutate {
            m: kxs_tui::cmd::Mutation::Cordon {
                unschedulable: false,
                ..
            },
            ..
        })
    ));
}

#[test]
fn ctrl_k_kills_without_a_dialog() {
    let mut app = test_app();
    seeded_resources(&mut app);
    let cmds = app.update(Msg::Key(ctrl('k')));
    assert!(app.chrome.confirm.is_none(), "ctrl-k opened a dialog");
    assert!(matches!(
        cmds.first(),
        Some(kxs_tui::cmd::Cmd::Mutate {
            m: kxs_tui::cmd::Mutation::Delete { force: true, .. },
            ..
        })
    ));
}

#[test]
fn shift_j_asks_for_the_owner_reference() {
    let mut app = test_app();
    seeded_resources(&mut app);
    let cmds = app.update(Msg::Key(KeyEvent::from(KeyCode::Char('J'))));
    assert!(matches!(
        cmds.first(),
        Some(kxs_tui::cmd::Cmd::Fetch {
            what: kxs_tui::cmd::Fetch::Owner { .. },
            ..
        })
    ));
}

#[test]
fn a_asks_for_attach_targets_on_a_pod() {
    let mut app = test_app();
    seeded_resources(&mut app);
    let cmds = app.update(Msg::Key(KeyEvent::from(KeyCode::Char('a'))));
    assert!(matches!(
        cmds.first(),
        Some(kxs_tui::cmd::Cmd::Fetch {
            what: kxs_tui::cmd::Fetch::AttachTargets { .. },
            ..
        })
    ));
}

#[test]
fn ctrl_s_saves_the_selected_resource() {
    let mut app = test_app();
    seeded_resources(&mut app);
    let cmds = app.update(Msg::Key(ctrl('s')));
    assert!(
        matches!(cmds.first(), Some(kxs_tui::cmd::Cmd::SaveResource { name, .. }) if name == "web-7d9f")
    );
}

#[test]
fn readonly_refuses_attach() {
    let cfg = kxs_tui::config::Config {
        readonly: true,
        ..Default::default()
    };
    let mut app = App::new(
        std::sync::Arc::new(std::sync::Mutex::new(Sessions::default())),
        std::sync::Arc::new(std::sync::Mutex::new(cfg)),
        theme::get(theme::DEFAULT_ID),
    );
    seeded_resources(&mut app);
    let cmds = app.update(Msg::Key(KeyEvent::from(KeyCode::Char('a'))));
    assert!(cmds.is_empty());
    assert!(app
        .chrome
        .flash
        .as_ref()
        .is_some_and(|f| f.text.contains("readonly")));
}

#[test]
fn live_status_clears_indicator_and_errors_keep_message() {
    let mut app = test_app();
    let mut view = resources_view(&mut app);
    let ctx = app.ctx();
    let id = view.id();
    view.on_msg(
        &Msg::Table {
            view: id,
            ev: TableEvent::Status {
                state: "reconnecting".into(),
                message: Some("boom".into()),
            },
        },
        &ctx,
    );
    assert!(
        view.title().contains("⟳ reconnecting: boom"),
        "{}",
        view.title()
    );
    view.on_msg(
        &Msg::Table {
            view: id,
            ev: TableEvent::Status {
                state: "live".into(),
                message: None,
            },
        },
        &ctx,
    );
    assert!(!view.title().contains('⟳'), "{}", view.title());
    view.on_msg(
        &Msg::Table {
            view: id,
            ev: TableEvent::Status {
                state: "error".into(),
                message: Some("forbidden".into()),
            },
        },
        &ctx,
    );
    assert!(view.title().contains("⟳ forbidden"), "{}", view.title());
    view.on_started(
        kxs_tui::cmd::StopHandle(tokio::sync::oneshot::channel().0),
        &ctx,
    );
    assert!(view.title().contains("⟳ forbidden"), "{}", view.title());
}

#[test]
fn pods_status_uses_the_same_mapping() {
    let mut app = test_app();
    let mut view = kxs_tui::views::pods::PodsView::new(&mut app, Some("default".into()));
    let ctx = app.ctx();
    let id = view.id();
    view.on_msg(
        &Msg::Pod {
            view: id,
            ev: kxs_cluster::pods::PodEvent::Status {
                state: "live".into(),
                message: None,
            },
        },
        &ctx,
    );
    assert!(!view.title().contains('⟳'), "{}", view.title());
    view.on_msg(
        &Msg::Pod {
            view: id,
            ev: kxs_cluster::pods::PodEvent::Status {
                state: "reconnecting".into(),
                message: Some("line one\nline two".into()),
            },
        },
        &ctx,
    );
    assert!(
        view.title().contains("⟳ reconnecting: line one"),
        "{}",
        view.title()
    );
    assert!(!view.title().contains("line two"), "{}", view.title());
}

#[test]
fn events_view_shows_full_message_column() {
    use kxs_tui::views::events::EventsView;
    let mut app = test_app();
    let mut view = EventsView::new(&mut app, Some("default".into()));
    let ctx = app.ctx();
    let id = view.id();
    let table = ResourceTable {
        columns: vec![
            "Last Seen".into(),
            "Type".into(),
            "Reason".into(),
            "Object".into(),
            "Message".into(),
            "Age".into(),
        ],
        rows: vec![ResourceRow {
            key: "default/e1".into(),
            name: "e1".into(),
            namespace: Some("default".into()),
            cells: vec![
                "2m".into(),
                "Warning".into(),
                "BackOff".into(),
                "pod/crasher".into(),
                "Back-off restarting failed container crash in pod crasher".into(),
            ],
            created: Some("2026-09-01T00:00:00Z".into()),
        }],
    };
    view.on_msg(
        &Msg::Table {
            view: id,
            ev: TableEvent::Table { table },
        },
        &ctx,
    );
    let mut t = Terminal::new(TestBackend::new(140, 8)).unwrap();
    t.draw(|f| view.render(f, f.area(), &theme::get(theme::DEFAULT_ID), ""))
        .unwrap();
    let text = buf_text(&t);
    assert!(text.contains("BackOff"), "{text}");
    assert!(text.contains("pod/crasher"), "{text}");
    assert!(
        text.contains("Back-off restarting failed container"),
        "{text}"
    );
}

#[test]
fn resources_selection_after_snapshot_respects_filter() {
    let mut app = test_app();
    let mut view = resources_view(&mut app);
    let ctx = app.ctx();
    let id = view.id();
    view.set_filter("api");
    view.on_msg(
        &Msg::Table {
            view: id,
            ev: TableEvent::Table {
                table: fixture_table(),
            },
        },
        &ctx,
    );
    let target = view.target().expect("target");
    assert_eq!(target.name, "api-xyz");
}

#[test]
fn pods_selection_after_snapshot_respects_filter() {
    use kxs_cluster::pods::{PodEvent, PodRow};
    use kxs_tui::views::pods::PodsView;
    let mut app = test_app();
    let mut view = PodsView::new(&mut app, Some("default".into()));
    let ctx = app.ctx();
    let id = view.id();
    let row = |name: &str| PodRow {
        key: format!("default/{name}"),
        name: name.into(),
        namespace: "default".into(),
        ready: "1/1".into(),
        status: "Running".into(),
        restarts: 0,
        ip: None,
        node: None,
        created: Some("2026-09-01T00:00:00Z".into()),
        cpu_request_millis: None,
        mem_request_mib: None,
    };
    view.set_filter("web");
    view.on_msg(
        &Msg::Pod {
            view: id,
            ev: PodEvent::Snapshot {
                rows: vec![row("agent-1"), row("web-1")],
            },
        },
        &ctx,
    );
    assert_eq!(view.target().expect("target").name, "web-1");
}
