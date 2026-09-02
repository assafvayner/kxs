//! Golden tests for `kxs_cluster::describe::describe_value`.
//!
//! Fixtures live in `tests/fixtures/describe/<name>.json` (an object as the
//! API server returns it) with the expected output in `<name>.txt`. Both
//! sides are normalized before comparison: trailing whitespace trimmed,
//! leading whitespace kept, internal runs of 2+ spaces collapsed to two.
//! Run with `UPDATE_GOLDENS=1` to rewrite the `.txt` files from actual output.

use k8s_openapi::chrono::{DateTime, Utc};
use kxs_cluster::describe::{describe_value, Lookups};
use kxs_cluster::discovery::ResourceKind;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn now() -> DateTime<Utc> {
    "2026-07-03T12:00:00Z".parse().unwrap()
}

fn kind(group: &str, version: &str, kind: &str, plural: &str, namespaced: bool) -> ResourceKind {
    ResourceKind {
        group: group.into(),
        version: version.into(),
        kind: kind.into(),
        plural: plural.into(),
        namespaced,
        aliases: vec![],
    }
}

fn normalize(s: &str) -> String {
    let mut out = String::new();
    for line in s.lines() {
        let line = line.trim_end();
        let lead = line.len() - line.trim_start().len();
        out.push_str(&line[..lead]);
        let mut run = 0;
        for ch in line[lead..].chars() {
            if ch == ' ' {
                run += 1;
                continue;
            }
            if run > 0 {
                out.push_str(if run >= 2 { "  " } else { " " });
                run = 0;
            }
            out.push(ch);
        }
        out.push('\n');
    }
    out
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/describe")
}

fn golden(name: &str, kind: &ResourceKind, lookups: &Lookups) {
    let dir = fixtures();
    let raw = fs::read_to_string(dir.join(format!("{name}.json")))
        .unwrap_or_else(|e| panic!("read {name}.json: {e}"));
    let value: Value = serde_json::from_str(&raw).unwrap();
    let actual = normalize(&describe_value(kind, &value, lookups, &[], now()));
    let path = dir.join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_GOLDENS").is_some() {
        fs::write(&path, &actual).unwrap();
        return;
    }
    let expected =
        normalize(&fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}.txt: {e}")));
    assert_eq!(
        actual, expected,
        "golden mismatch for {name} (UPDATE_GOLDENS=1 to accept)"
    );
}

#[test]
fn generic_crd_instance() {
    golden(
        "widget",
        &kind("example.com", "v1", "Widget", "widgets", true),
        &Lookups::default(),
    );
}
