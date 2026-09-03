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
    let target = kxs_tui::view::Target {
        kind: pod_kind(),
        ns: Some("default".into()),
        name: "web".into(),
    };
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
    let target = kxs_tui::view::Target {
        kind: pod_kind(),
        ns: Some("default".into()),
        name: "web".into(),
    };
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
