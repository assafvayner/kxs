//! Golden tests for `kxs_cluster::describe::describe_value`.
//!
//! Fixtures live in `tests/fixtures/describe/<name>.json` (an object as the
//! API server returns it) with the expected output in `<name>.txt`. Both
//! sides are normalized before comparison: trailing whitespace trimmed,
//! leading whitespace kept, internal runs of 2+ spaces collapsed to two.
//! Run with `UPDATE_GOLDENS=1` to rewrite the `.txt` files from actual output.

use k8s_openapi::api::coordination::v1::{Lease, LeaseSpec};
use k8s_openapi::api::core::v1::{
    Container, EndpointAddress, EndpointPort, EndpointSubset, Endpoints, Pod, PodSpec,
    ResourceRequirements,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{MicroTime, ObjectMeta, Time};
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

fn quantities(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<String, Quantity> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), Quantity(v.to_string())))
        .collect()
}

#[test]
fn generic_crd_instance() {
    golden(
        "widget",
        &kind("example.com", "v1", "Widget", "widgets", true),
        &Lookups::default(),
    );
}

#[test]
fn pod() {
    golden(
        "pod",
        &kind("", "v1", "Pod", "pods", true),
        &Lookups::default(),
    );
}

#[test]
fn deployment() {
    golden(
        "deployment",
        &kind("apps", "v1", "Deployment", "deployments", true),
        &Lookups::default(),
    );
}

#[test]
fn replicaset() {
    golden(
        "replicaset",
        &kind("apps", "v1", "ReplicaSet", "replicasets", true),
        &Lookups::default(),
    );
}

#[test]
fn statefulset() {
    golden(
        "statefulset",
        &kind("apps", "v1", "StatefulSet", "statefulsets", true),
        &Lookups::default(),
    );
}

#[test]
fn daemonset() {
    golden(
        "daemonset",
        &kind("apps", "v1", "DaemonSet", "daemonsets", true),
        &Lookups::default(),
    );
}

#[test]
fn job() {
    golden(
        "job",
        &kind("batch", "v1", "Job", "jobs", true),
        &Lookups::default(),
    );
}

#[test]
fn cronjob() {
    golden(
        "cronjob",
        &kind("batch", "v1", "CronJob", "cronjobs", true),
        &Lookups::default(),
    );
}

#[test]
fn service_with_endpoints() {
    let endpoints = Endpoints {
        subsets: Some(vec![EndpointSubset {
            addresses: Some(vec![
                EndpointAddress {
                    ip: "10.0.1.4".into(),
                    ..Default::default()
                },
                EndpointAddress {
                    ip: "10.0.1.5".into(),
                    ..Default::default()
                },
            ]),
            ports: Some(vec![EndpointPort {
                name: Some("http".into()),
                port: 8080,
                ..Default::default()
            }]),
            ..Default::default()
        }]),
        ..Default::default()
    };
    let lookups = Lookups {
        endpoints: Some(endpoints),
        ..Default::default()
    };
    golden(
        "service",
        &kind("", "v1", "Service", "services", true),
        &lookups,
    );
}

#[test]
fn endpoints() {
    golden(
        "endpoints",
        &kind("", "v1", "Endpoints", "endpoints", true),
        &Lookups::default(),
    );
}

#[test]
fn ingress() {
    golden(
        "ingress",
        &kind("networking.k8s.io", "v1", "Ingress", "ingresses", true),
        &Lookups::default(),
    );
}

#[test]
fn configmap() {
    golden(
        "configmap",
        &kind("", "v1", "ConfigMap", "configmaps", true),
        &Lookups::default(),
    );
}

#[test]
fn secret() {
    golden(
        "secret",
        &kind("", "v1", "Secret", "secrets", true),
        &Lookups::default(),
    );
}

#[test]
fn node_with_pods() {
    let pod = Pod {
        metadata: ObjectMeta {
            name: Some("web-1".into()),
            namespace: Some("default".into()),
            creation_timestamp: Some(Time("2026-07-01T00:00:00Z".parse().unwrap())),
            ..Default::default()
        },
        spec: Some(PodSpec {
            containers: vec![Container {
                name: "web".into(),
                resources: Some(ResourceRequirements {
                    requests: Some(quantities(&[("cpu", "100m"), ("memory", "64Mi")])),
                    limits: Some(quantities(&[("cpu", "500m"), ("memory", "128Mi")])),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let lease = Lease {
        spec: Some(LeaseSpec {
            holder_identity: Some("node-a".into()),
            acquire_time: Some(MicroTime("2026-07-01T00:00:00Z".parse().unwrap())),
            renew_time: Some(MicroTime("2026-07-03T11:59:00Z".parse().unwrap())),
            ..Default::default()
        }),
        ..Default::default()
    };
    let lookups = Lookups {
        lease: Some(lease),
        pods: vec![pod],
        ..Default::default()
    };
    golden("node", &kind("", "v1", "Node", "nodes", false), &lookups);
}
