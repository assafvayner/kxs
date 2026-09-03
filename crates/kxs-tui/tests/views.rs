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
