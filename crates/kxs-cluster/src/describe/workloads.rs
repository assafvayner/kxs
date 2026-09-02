use super::header::{write_controlled_by, write_labels_annotations};
use super::pod::write_pod_template;
use super::util::{int_or_string, map_lines, or_none, rfc1123z, selector_string, write_list, NONE};
use super::writer::Writer;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet, StatefulSet};
use k8s_openapi::api::core::v1::{Pod, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

const POD_TEMPLATE_HASH: &str = "pod-template-hash";
const LEGACY_STORAGE_CLASS: &str = "volume.beta.kubernetes.io/storage-class";

/// `N Running / N Waiting / N Succeeded / N Failed` over the given pods.
pub fn pods_status(pods: &[Pod], owner_uid: Option<&str>) -> String {
    let (mut running, mut waiting, mut succeeded, mut failed) = (0, 0, 0, 0);
    let Some(owner_uid) = owner_uid else {
        return format!(
            "{running} Running / {waiting} Waiting / {succeeded} Succeeded / {failed} Failed"
        );
    };
    for p in pods {
        if !controlled_by(&p.metadata, owner_uid) {
            continue;
        }
        match p.status.as_ref().and_then(|s| s.phase.as_deref()) {
            Some("Running") => running += 1,
            Some("Pending") => waiting += 1,
            Some("Succeeded") => succeeded += 1,
            Some("Failed") => failed += 1,
            _ => {}
        }
    }
    format!("{running} Running / {waiting} Waiting / {succeeded} Succeeded / {failed} Failed")
}

fn write_conditions<'a, I>(w: &mut Writer, conds: I)
where
    I: IntoIterator<Item = (&'a str, &'a str, &'a str)>,
{
    let rows: Vec<_> = conds.into_iter().collect();
    if rows.is_empty() {
        return;
    }
    w.section(0, "Conditions");
    w.cells(1, &["Type", "Status", "Reason"]);
    w.cells(1, &["----", "------", "------"]);
    for (t, s, r) in rows {
        w.cells(1, &[t, s, r]);
    }
}

fn controlled_by(child: &ObjectMeta, owner_uid: &str) -> bool {
    child
        .owner_references
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .any(|o| o.controller == Some(true) && o.uid == owner_uid)
}

fn owned_by(child: &ObjectMeta, owner: &ObjectMeta) -> bool {
    owner
        .uid
        .as_deref()
        .is_some_and(|uid| controlled_by(child, uid))
}

fn templates_equal_ignoring_hash(a: &PodTemplateSpec, b: &PodTemplateSpec) -> bool {
    fn without_hash(mut template: PodTemplateSpec) -> PodTemplateSpec {
        if let Some(labels) = template
            .metadata
            .as_mut()
            .and_then(|meta| meta.labels.as_mut())
        {
            labels.remove(POD_TEMPLATE_HASH);
        }
        template
    }

    without_hash(a.clone()) == without_hash(b.clone())
}

pub fn write_deployment(w: &mut Writer, d: &Deployment, replica_sets: &[ReplicaSet]) {
    let meta = &d.metadata;
    let spec = d.spec.as_ref();
    let status = d.status.as_ref();
    w.kv(0, "Name", or_none(meta.name.as_deref()));
    w.kv(0, "Namespace", or_none(meta.namespace.as_deref()));
    if let Some(t) = &meta.creation_timestamp {
        w.kv(0, "CreationTimestamp", rfc1123z(t));
    }
    write_labels_annotations(w, meta);
    w.kv(
        0,
        "Selector",
        spec.map(|s| selector_string(&s.selector))
            .unwrap_or_else(|| NONE.into()),
    );
    let n = |f: fn(&k8s_openapi::api::apps::v1::DeploymentStatus) -> Option<i32>| {
        status.and_then(f).unwrap_or(0)
    };
    w.kv(
        0,
        "Replicas",
        format!(
            "{} desired | {} updated | {} total | {} available | {} unavailable",
            spec.and_then(|s| s.replicas).unwrap_or(1),
            n(|s| s.updated_replicas),
            n(|s| s.replicas),
            n(|s| s.available_replicas),
            n(|s| s.unavailable_replicas)
        ),
    );
    let strategy = spec.and_then(|s| s.strategy.as_ref());
    w.kv(
        0,
        "StrategyType",
        or_none(strategy.and_then(|s| s.type_.as_deref())),
    );
    w.kv(
        0,
        "MinReadySeconds",
        spec.and_then(|s| s.min_ready_seconds).unwrap_or(0),
    );
    if let Some(ru) = strategy.and_then(|s| s.rolling_update.as_ref()) {
        let mu = ru
            .max_unavailable
            .as_ref()
            .map(int_or_string)
            .unwrap_or_else(|| "25%".into());
        let ms = ru
            .max_surge
            .as_ref()
            .map(int_or_string)
            .unwrap_or_else(|| "25%".into());
        w.kv(
            0,
            "RollingUpdateStrategy",
            format!("{mu} max unavailable, {ms} max surge"),
        );
    }
    if let Some(s) = spec {
        write_pod_template(w, 0, &s.template);
    }
    write_conditions(
        w,
        status
            .and_then(|s| s.conditions.as_deref())
            .unwrap_or(&[])
            .iter()
            .map(|c| {
                (
                    c.type_.as_str(),
                    c.status.as_str(),
                    c.reason.as_deref().unwrap_or(""),
                )
            }),
    );
    let mut owned: Vec<&ReplicaSet> = replica_sets
        .iter()
        .filter(|rs| owned_by(&rs.metadata, meta))
        .collect();
    if owned.is_empty() {
        return;
    }
    owned.sort_by(|a, b| {
        a.metadata
            .creation_timestamp
            .cmp(&b.metadata.creation_timestamp)
            .then_with(|| a.metadata.name.cmp(&b.metadata.name))
    });
    let new_index = spec.and_then(|deployment_spec| {
        owned.iter().position(|rs| {
            rs.spec
                .as_ref()
                .and_then(|rs_spec| rs_spec.template.as_ref())
                .is_some_and(|template| {
                    templates_equal_ignoring_hash(&deployment_spec.template, template)
                })
        })
    });
    let fmt_rs = |rs: &ReplicaSet| {
        format!(
            "{} ({}/{} replicas created)",
            rs.metadata.name.as_deref().unwrap_or(""),
            rs.status.as_ref().map(|s| s.replicas).unwrap_or(0),
            rs.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0)
        )
    };
    let old: Vec<&ReplicaSet> = owned
        .iter()
        .enumerate()
        .filter_map(|(index, rs)| (Some(index) != new_index).then_some(*rs))
        .collect();
    let old_line = if old.is_empty() {
        NONE.to_string()
    } else {
        old.iter()
            .map(|rs| fmt_rs(rs))
            .collect::<Vec<_>>()
            .join(", ")
    };
    w.kv(0, "OldReplicaSets", old_line);
    w.kv(
        0,
        "NewReplicaSet",
        new_index
            .map(|index| fmt_rs(owned[index]))
            .unwrap_or_else(|| NONE.into()),
    );
}

pub fn write_replicaset(w: &mut Writer, rs: &ReplicaSet, pods: &[Pod]) {
    let meta = &rs.metadata;
    let spec = rs.spec.as_ref();
    w.kv(0, "Name", or_none(meta.name.as_deref()));
    w.kv(0, "Namespace", or_none(meta.namespace.as_deref()));
    w.kv(
        0,
        "Selector",
        spec.map(|s| selector_string(&s.selector))
            .unwrap_or_else(|| NONE.into()),
    );
    write_labels_annotations(w, meta);
    write_controlled_by(w, meta);
    w.kv(
        0,
        "Replicas",
        format!(
            "{} current / {} desired",
            rs.status.as_ref().map(|s| s.replicas).unwrap_or(0),
            spec.and_then(|s| s.replicas).unwrap_or(1)
        ),
    );
    w.kv(0, "Pods Status", pods_status(pods, meta.uid.as_deref()));
    if let Some(t) = spec.and_then(|s| s.template.as_ref()) {
        write_pod_template(w, 0, t);
    }
    write_conditions(
        w,
        rs.status
            .as_ref()
            .and_then(|s| s.conditions.as_deref())
            .unwrap_or(&[])
            .iter()
            .map(|c| {
                (
                    c.type_.as_str(),
                    c.status.as_str(),
                    c.reason.as_deref().unwrap_or(""),
                )
            }),
    );
}

pub fn write_statefulset(w: &mut Writer, sts: &StatefulSet, pods: &[Pod]) {
    let meta = &sts.metadata;
    let spec = sts.spec.as_ref();
    w.kv(0, "Name", or_none(meta.name.as_deref()));
    w.kv(0, "Namespace", or_none(meta.namespace.as_deref()));
    if let Some(t) = &meta.creation_timestamp {
        w.kv(0, "CreationTimestamp", rfc1123z(t));
    }
    w.kv(
        0,
        "Selector",
        spec.map(|s| selector_string(&s.selector))
            .unwrap_or_else(|| NONE.into()),
    );
    write_labels_annotations(w, meta);
    w.kv(
        0,
        "Replicas",
        format!(
            "{} desired | {} total",
            spec.and_then(|s| s.replicas).unwrap_or(1),
            sts.status.as_ref().map(|s| s.replicas).unwrap_or(0)
        ),
    );
    let strategy = spec.and_then(|s| s.update_strategy.as_ref());
    w.kv(
        0,
        "Update Strategy",
        or_none(strategy.and_then(|s| s.type_.as_deref())),
    );
    if let Some(ru) = strategy.and_then(|s| s.rolling_update.as_ref()) {
        if let Some(partition) = ru.partition {
            w.kv(1, "Partition", partition);
        }
        if let Some(max_unavailable) = ru.max_unavailable.as_ref() {
            w.kv(1, "MaxUnavailable", int_or_string(max_unavailable));
        }
    }
    w.kv(0, "Pods Status", pods_status(pods, meta.uid.as_deref()));
    if let Some(s) = spec {
        write_pod_template(w, 0, &s.template);
    }
    let claims = spec
        .and_then(|s| s.volume_claim_templates.as_deref())
        .unwrap_or(&[]);
    if claims.is_empty() {
        w.kv(0, "Volume Claims", NONE);
    } else {
        w.section(0, "Volume Claims");
        for pvc in claims {
            let pspec = pvc.spec.as_ref();
            w.kv(1, "Name", or_none(pvc.metadata.name.as_deref()));
            w.kv(
                1,
                "StorageClass",
                pvc.metadata
                    .annotations
                    .as_ref()
                    .and_then(|a| a.get(LEGACY_STORAGE_CLASS))
                    .map(String::as_str)
                    .or_else(|| pspec.and_then(|s| s.storage_class_name.as_deref()))
                    .unwrap_or(""),
            );
            write_list(w, 1, "Labels", &map_lines(pvc.metadata.labels.as_ref()));
            write_list(
                w,
                1,
                "Annotations",
                &map_lines(pvc.metadata.annotations.as_ref()),
            );
            let capacity = pspec
                .and_then(|s| s.resources.as_ref())
                .and_then(|r| r.requests.as_ref())
                .and_then(|r| r.get("storage"))
                .map(|q| q.0.as_str())
                .unwrap_or("<default>");
            w.kv(1, "Capacity", capacity);
            let modes = pspec
                .and_then(|s| s.access_modes.as_deref())
                .unwrap_or(&[])
                .join(" ");
            w.kv(1, "Access Modes", format!("[{modes}]"));
        }
    }
}

pub fn write_daemonset(w: &mut Writer, ds: &DaemonSet, pods: &[Pod]) {
    let meta = &ds.metadata;
    let spec = ds.spec.as_ref();
    let status = ds.status.as_ref();
    w.kv(0, "Name", or_none(meta.name.as_deref()));
    w.kv(
        0,
        "Selector",
        spec.map(|s| selector_string(&s.selector))
            .unwrap_or_else(|| NONE.into()),
    );
    let node_selector = spec
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|p| p.node_selector.as_ref());
    write_list(w, 0, "Node-Selector", &map_lines(node_selector));
    write_labels_annotations(w, meta);
    w.kv(
        0,
        "Desired Number of Nodes Scheduled",
        status.map(|s| s.desired_number_scheduled).unwrap_or(0),
    );
    w.kv(
        0,
        "Current Number of Nodes Scheduled",
        status.map(|s| s.current_number_scheduled).unwrap_or(0),
    );
    w.kv(
        0,
        "Number of Nodes Scheduled with Up-to-date Pods",
        status.and_then(|s| s.updated_number_scheduled).unwrap_or(0),
    );
    w.kv(
        0,
        "Number of Nodes Scheduled with Available Pods",
        status.and_then(|s| s.number_available).unwrap_or(0),
    );
    w.kv(
        0,
        "Number of Nodes Misscheduled",
        status.map(|s| s.number_misscheduled).unwrap_or(0),
    );
    w.kv(0, "Pods Status", pods_status(pods, meta.uid.as_deref()));
    if let Some(s) = spec {
        write_pod_template(w, 0, &s.template);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pod(phase: Option<&str>, owner_uid: &str, controller: bool) -> Pod {
        serde_json::from_value(json!({
            "metadata": {
                "ownerReferences": [{
                    "apiVersion": "apps/v1",
                    "kind": "ReplicaSet",
                    "name": "owner",
                    "uid": owner_uid,
                    "controller": controller
                }]
            },
            "status": {"phase": phase}
        }))
        .unwrap()
    }

    #[test]
    fn pods_status_counts_known_phases_for_controller_owned_pods_only() {
        let pods = vec![
            pod(Some("Running"), "workload", true),
            pod(Some("Pending"), "workload", true),
            pod(Some("Succeeded"), "workload", true),
            pod(Some("Failed"), "workload", true),
            pod(Some("Unknown"), "workload", true),
            pod(None, "workload", true),
            pod(Some("Running"), "foreign", true),
            pod(Some("Running"), "workload", false),
        ];
        assert_eq!(
            pods_status(&pods, Some("workload")),
            "1 Running / 1 Waiting / 1 Succeeded / 1 Failed"
        );
        assert_eq!(
            pods_status(&pods, None),
            "0 Running / 0 Waiting / 0 Succeeded / 0 Failed"
        );
    }

    fn deployment() -> Deployment {
        serde_json::from_value(json!({
            "metadata": {"name": "web", "namespace": "default", "uid": "deployment"},
            "spec": {
                "selector": {"matchLabels": {"app": "web"}},
                "template": {
                    "metadata": {"labels": {"app": "web"}},
                    "spec": {"containers": [{"name": "web", "image": "nginx"}]}
                }
            }
        }))
        .unwrap()
    }

    fn replica_set(
        name: &str,
        timestamp: &str,
        app: &str,
        owner_uid: &str,
        controller: bool,
    ) -> ReplicaSet {
        serde_json::from_value(json!({
            "metadata": {
                "name": name,
                "creationTimestamp": timestamp,
                "ownerReferences": [{
                    "apiVersion": "apps/v1",
                    "kind": "Deployment",
                    "name": "web",
                    "uid": owner_uid,
                    "controller": controller
                }]
            },
            "spec": {
                "replicas": 1,
                "selector": {"matchLabels": {"app": app}},
                "template": {
                    "metadata": {"labels": {"app": app, "pod-template-hash": name}},
                    "spec": {"containers": [{"name": "web", "image": "nginx"}]}
                }
            },
            "status": {"replicas": 1}
        }))
        .unwrap()
    }

    #[test]
    fn deployment_selects_oldest_matching_template_and_sorts_remaining_owned_sets() {
        let deployment = deployment();
        let replica_sets = vec![
            replica_set(
                "matching-b",
                "2026-07-02T00:00:00Z",
                "web",
                "deployment",
                true,
            ),
            replica_set("foreign", "2026-06-01T00:00:00Z", "web", "other", true),
            replica_set(
                "not-controller",
                "2026-06-01T00:00:00Z",
                "web",
                "deployment",
                false,
            ),
            replica_set(
                "different",
                "2026-07-01T00:00:00Z",
                "other",
                "deployment",
                true,
            ),
            replica_set(
                "matching-a",
                "2026-07-02T00:00:00Z",
                "web",
                "deployment",
                true,
            ),
            replica_set(
                "matching-newer",
                "2026-07-03T00:00:00Z",
                "web",
                "deployment",
                true,
            ),
        ];
        let mut w = Writer::new();
        write_deployment(&mut w, &deployment, &replica_sets);
        let output = w.finish();

        let old = output
            .lines()
            .find(|line| line.starts_with("OldReplicaSets:"))
            .unwrap();
        assert!(old.contains(
            "different (1/1 replicas created), matching-b (1/1 replicas created), matching-newer (1/1 replicas created)"
        ));
        assert!(!old.contains("foreign"));
        assert!(!old.contains("not-controller"));
        let new = output
            .lines()
            .find(|line| line.starts_with("NewReplicaSet:"))
            .unwrap();
        assert!(new.contains("matching-a (1/1 replicas created)"));
    }

    #[test]
    fn deployment_omits_replica_set_lines_when_lookup_is_empty() {
        let mut w = Writer::new();
        write_deployment(&mut w, &deployment(), &[]);
        let output = w.finish();
        assert!(!output.contains("OldReplicaSets:"));
        assert!(!output.contains("NewReplicaSet:"));
    }

    #[test]
    fn statefulset_prints_only_present_strategy_fields_and_legacy_claim_defaults() {
        let statefulset: StatefulSet = serde_json::from_value(json!({
            "metadata": {"name": "db", "namespace": "default", "uid": "statefulset"},
            "spec": {
                "replicas": 1,
                "selector": {"matchLabels": {"app": "db"}},
                "serviceName": "db",
                "updateStrategy": {
                    "type": "RollingUpdate",
                    "rollingUpdate": {"maxUnavailable": "25%"}
                },
                "template": {
                    "metadata": {"labels": {"app": "db"}},
                    "spec": {"containers": [{"name": "db", "image": "postgres"}]}
                },
                "volumeClaimTemplates": [{
                    "metadata": {
                        "name": "data",
                        "annotations": {"volume.beta.kubernetes.io/storage-class": "legacy"}
                    },
                    "spec": {
                        "storageClassName": "modern",
                        "accessModes": ["ReadWriteOnce"]
                    }
                }, {
                    "metadata": {"name": "unclassified"},
                    "spec": {"accessModes": ["ReadWriteOnce"]}
                }]
            },
            "status": {"replicas": 1}
        }))
        .unwrap();
        let mut w = Writer::new();
        write_statefulset(&mut w, &statefulset, &[]);
        let output = w.finish();

        assert!(!output.contains("Partition:"));
        assert!(output.contains("MaxUnavailable:  25%"));
        assert!(output.contains("StorageClass:  legacy"));
        assert!(!output.contains("modern"));
        assert!(output.lines().any(|line| line == "  StorageClass:"));
        let capacity = output
            .lines()
            .find(|line| line.trim_start().starts_with("Capacity:"))
            .unwrap();
        assert!(capacity.ends_with("<default>"));
    }
}
