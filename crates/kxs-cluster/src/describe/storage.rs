use super::header::write_labels_annotations;
use super::util::{access_modes_short, or_none, rfc1123z, write_list, NONE};
use super::writer::Writer;
use k8s_openapi::api::core::v1::{
    PersistentVolume, PersistentVolumeClaim, PersistentVolumeClaimCondition, Pod,
    TypedLocalObjectReference,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};
use kxs_core::format::human_duration;
use std::collections::{BTreeMap, BTreeSet};

const LEGACY_STORAGE_CLASS_ANNOTATION: &str = "volume.beta.kubernetes.io/storage-class";
const MAX_VOLUME_ATTRIBUTE_BYTES: usize = 256;

fn finalizers(finalizers: Option<&Vec<String>>) -> String {
    format!(
        "[{}]",
        finalizers
            .map(|values| values.join(" "))
            .unwrap_or_default()
    )
}

pub fn write_pvc(w: &mut Writer, pvc: &PersistentVolumeClaim, pods: &[Pod], now_ms: i64) {
    let metadata = &pvc.metadata;
    let spec = pvc.spec.as_ref();
    let status = pvc.status.as_ref();

    w.kv(0, "Name", or_none(metadata.name.as_deref()));
    w.kv(0, "Namespace", or_none(metadata.namespace.as_deref()));
    w.kv(
        0,
        "StorageClass",
        pvc_storage_class(
            metadata,
            spec.and_then(|spec| spec.storage_class_name.as_deref()),
        ),
    );
    w.kv(
        0,
        "Status",
        phase_with_termination(
            metadata.deletion_timestamp.as_ref(),
            status.and_then(|status| status.phase.as_deref()),
            now_ms,
        ),
    );
    w.kv(
        0,
        "Volume",
        spec.and_then(|spec| spec.volume_name.as_deref())
            .unwrap_or(""),
    );
    write_labels_annotations(w, metadata);
    w.kv(0, "Finalizers", finalizers(metadata.finalizers.as_ref()));

    let bound = spec
        .and_then(|spec| spec.volume_name.as_deref())
        .is_some_and(|name| !name.is_empty());
    let capacity = bound
        .then(|| status.and_then(|status| status.capacity.as_ref()))
        .flatten()
        .and_then(|capacity| capacity.get("storage"))
        .map(|quantity| quantity.0.as_str())
        .unwrap_or("");
    w.kv(0, "Capacity", capacity);
    let access_modes = bound
        .then(|| status.and_then(|status| status.access_modes.as_ref()))
        .flatten()
        .filter(|modes| !modes.is_empty())
        .map(|modes| access_modes_short(Some(modes)))
        .unwrap_or_default();
    w.kv(0, "Access Modes", access_modes);
    if let Some(volume_mode) = spec.and_then(|spec| spec.volume_mode.as_deref()) {
        w.kv(0, "VolumeMode", volume_mode);
    }
    if let Some(data_source) = spec.and_then(|spec| spec.data_source.as_ref()) {
        write_data_source(w, data_source);
    }

    let users = pvc_users(pvc, pods);
    write_list(w, 0, "Used By", &users);
    write_conditions(
        w,
        status
            .and_then(|status| status.conditions.as_deref())
            .unwrap_or(&[]),
    );
}

pub fn write_pv(w: &mut Writer, pv: &PersistentVolume, now_ms: i64) {
    let metadata = &pv.metadata;
    let spec = pv.spec.as_ref();
    let status = pv.status.as_ref();

    w.kv(0, "Name", or_none(metadata.name.as_deref()));
    write_labels_annotations(w, metadata);
    w.kv(0, "Finalizers", finalizers(metadata.finalizers.as_ref()));
    w.kv(
        0,
        "StorageClass",
        pv_storage_class(
            metadata,
            spec.and_then(|spec| spec.storage_class_name.as_deref()),
        ),
    );
    w.kv(
        0,
        "Status",
        phase_with_termination(
            metadata.deletion_timestamp.as_ref(),
            status.and_then(|status| status.phase.as_deref()),
            now_ms,
        ),
    );
    let claim = spec
        .and_then(|spec| spec.claim_ref.as_ref())
        .map(|reference| {
            format!(
                "{}/{}",
                reference.namespace.as_deref().unwrap_or(""),
                reference.name.as_deref().unwrap_or("")
            )
        })
        .unwrap_or_default();
    w.kv(0, "Claim", claim);
    w.kv(
        0,
        "Reclaim Policy",
        spec.and_then(|spec| spec.persistent_volume_reclaim_policy.as_deref())
            .unwrap_or(""),
    );
    w.kv(
        0,
        "Access Modes",
        access_modes_short(spec.and_then(|spec| spec.access_modes.as_ref())),
    );
    if let Some(volume_mode) = spec.and_then(|spec| spec.volume_mode.as_deref()) {
        w.kv(0, "VolumeMode", volume_mode);
    }
    let capacity = spec
        .and_then(|spec| spec.capacity.as_ref())
        .and_then(|capacity| capacity.get("storage"))
        .map(|quantity| quantity.0.as_str())
        .unwrap_or("");
    w.kv(0, "Capacity", capacity);

    if let Some(required) = spec
        .and_then(|spec| spec.node_affinity.as_ref())
        .and_then(|affinity| affinity.required.as_ref())
    {
        w.section(0, "Node Affinity");
        if required.node_selector_terms.is_empty() {
            w.kv(1, "Required Terms", NONE);
        } else {
            w.section(1, "Required Terms");
        }
        for (index, term) in required.node_selector_terms.iter().enumerate() {
            let expressions: Vec<String> = term
                .match_expressions
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(format_node_selector_requirement)
                .collect();
            let key = format!("Term {index}");
            if let Some((first, rest)) = expressions.split_first() {
                w.kv(2, &key, first);
                for expression in rest {
                    w.cont(2, expression);
                }
            } else {
                w.kv(2, &key, NONE);
            }
        }
    } else {
        w.kv(0, "Node Affinity", NONE);
    }

    w.kv(
        0,
        "Message",
        status
            .and_then(|status| status.message.as_deref())
            .unwrap_or(""),
    );
    w.section(0, "Source");
    write_source(w, pv);
}

fn write_source(w: &mut Writer, pv: &PersistentVolume) {
    let Some(spec) = pv.spec.as_ref() else {
        w.text(1, "<unknown>");
        return;
    };

    if let Some(csi) = spec.csi.as_ref() {
        w.kv(
            1,
            "Type",
            "CSI (a Container Storage Interface (CSI) volume source)",
        );
        w.kv(1, "Driver", &csi.driver);
        w.kv(1, "FSType", csi.fs_type.as_deref().unwrap_or(""));
        w.kv(1, "VolumeHandle", &csi.volume_handle);
        w.kv(1, "ReadOnly", csi.read_only.unwrap_or(false));
        let attributes: Vec<String> = csi
            .volume_attributes
            .as_ref()
            .map(|attributes| {
                attributes
                    .iter()
                    .map(|(key, value)| bounded_attribute(key, value))
                    .collect()
            })
            .unwrap_or_default();
        write_list(w, 1, "VolumeAttributes", &attributes);
    } else if let Some(host_path) = spec.host_path.as_ref() {
        w.kv(1, "Type", "HostPath (bare host directory volume)");
        w.kv(1, "Path", &host_path.path);
        w.kv(
            1,
            "HostPathType",
            host_path.type_.as_deref().unwrap_or(NONE),
        );
    } else if let Some(nfs) = spec.nfs.as_ref() {
        w.kv(
            1,
            "Type",
            "NFS (an NFS mount that lasts the lifetime of a pod)",
        );
        w.kv(1, "Server", &nfs.server);
        w.kv(1, "Path", &nfs.path);
        w.kv(1, "ReadOnly", nfs.read_only.unwrap_or(false));
    } else if let Some(local) = spec.local.as_ref() {
        w.kv(
            1,
            "Type",
            "LocalVolume (a persistent volume backed by local storage on a node)",
        );
        w.kv(1, "Path", &local.path);
    } else {
        // kubectl's broad source switch is intentionally out of scope for the
        // four source kinds supported by this describer.
        w.text(1, "<unknown>");
    }
}

fn pvc_storage_class<'a>(metadata: &'a ObjectMeta, class: Option<&'a str>) -> &'a str {
    metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get(LEGACY_STORAGE_CLASS_ANNOTATION))
        .map(String::as_str)
        .or(class)
        .unwrap_or("")
}

fn pv_storage_class<'a>(metadata: &'a ObjectMeta, class: Option<&'a str>) -> &'a str {
    metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get(LEGACY_STORAGE_CLASS_ANNOTATION))
        .map(String::as_str)
        .or(class)
        .unwrap_or("")
}

fn phase_with_termination(
    deletion_timestamp: Option<&Time>,
    phase: Option<&str>,
    now_ms: i64,
) -> String {
    if let Some(deletion_timestamp) = deletion_timestamp {
        let seconds = (now_ms - deletion_timestamp.0.timestamp_millis()) / 1000;
        format!("Terminating (lasts {})", human_duration(seconds))
    } else {
        phase.unwrap_or("").to_string()
    }
}

fn write_conditions(w: &mut Writer, conditions: &[PersistentVolumeClaimCondition]) {
    if conditions.is_empty() {
        return;
    }

    w.section(0, "Conditions");
    w.cells(
        1,
        &[
            "Type",
            "Status",
            "LastProbeTime",
            "LastTransitionTime",
            "Reason",
            "Message",
        ],
    );
    w.cells(
        1,
        &[
            "----",
            "------",
            "-----------------",
            "------------------",
            "------",
            "-------",
        ],
    );
    for condition in conditions {
        let probe = timestamp_or_unknown(condition.last_probe_time.as_ref());
        let transition = timestamp_or_unknown(condition.last_transition_time.as_ref());
        w.cells(
            1,
            &[
                &condition.type_,
                &condition.status,
                &probe,
                &transition,
                condition.reason.as_deref().unwrap_or(""),
                condition.message.as_deref().unwrap_or(""),
            ],
        );
    }
}

fn timestamp_or_unknown(timestamp: Option<&Time>) -> String {
    timestamp
        .map(rfc1123z)
        .unwrap_or_else(|| "<unknown>".to_string())
}

fn write_data_source(w: &mut Writer, data_source: &TypedLocalObjectReference) {
    w.section(0, "DataSource");
    if let Some(api_group) = data_source.api_group.as_deref() {
        w.kv(1, "APIGroup", api_group);
    }
    w.kv(1, "Kind", &data_source.kind);
    w.kv(1, "Name", &data_source.name);
}

fn pvc_users(pvc: &PersistentVolumeClaim, pods: &[Pod]) -> Vec<String> {
    let namespace = pvc.metadata.namespace.as_deref();
    let claim_name = pvc.metadata.name.as_deref().unwrap_or("");
    let pod_owner_uids: BTreeSet<&str> = pvc
        .metadata
        .owner_references
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter(|owner| owner.kind == "Pod")
        .map(|owner| owner.uid.as_str())
        .collect();
    let mut users = BTreeMap::new();

    for pod in pods {
        if pod.metadata.namespace.as_deref() != namespace {
            continue;
        }
        let directly_mounted = pod
            .spec
            .as_ref()
            .and_then(|spec| spec.volumes.as_ref())
            .is_some_and(|volumes| {
                volumes.iter().any(|volume| {
                    volume
                        .persistent_volume_claim
                        .as_ref()
                        .is_some_and(|claim| claim.claim_name == claim_name)
                })
            });
        let owned_for_pod = pod
            .metadata
            .uid
            .as_deref()
            .is_some_and(|uid| pod_owner_uids.contains(uid));
        if !directly_mounted && !owned_for_pod {
            continue;
        }
        let Some(name) = pod.metadata.name.as_ref() else {
            continue;
        };
        let identity = pod
            .metadata
            .uid
            .as_ref()
            .map(|uid| format!("uid:{uid}"))
            .unwrap_or_else(|| format!("name:{name}"));
        users.entry(identity).or_insert_with(|| name.clone());
    }

    let mut names: Vec<String> = users.into_values().collect();
    names.sort();
    names
}

fn format_node_selector_requirement(
    requirement: &k8s_openapi::api::core::v1::NodeSelectorRequirement,
) -> String {
    let mut result = format!(
        "{} {}",
        requirement.key,
        requirement.operator.to_lowercase()
    );
    if let Some(values) = requirement
        .values
        .as_deref()
        .filter(|values| !values.is_empty())
    {
        result.push_str(&format!(" [{}]", values.join(", ")));
    }
    result
}

fn bounded_attribute(key: &str, value: &str) -> String {
    let line = format!("{key}={value}");
    if line.len() > MAX_VOLUME_ATTRIBUTE_BYTES {
        let mut end = MAX_VOLUME_ATTRIBUTE_BYTES;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &line[..end])
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const NOW_MS: i64 = 1_783_080_000_000;

    fn from_value<T: serde::de::DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value).unwrap()
    }

    fn normalize(value: &str) -> String {
        let mut output = String::new();
        for line in value.lines() {
            let line = line.trim_end();
            let leading = line.len() - line.trim_start().len();
            output.push_str(&line[..leading]);
            let mut spaces = 0;
            for character in line[leading..].chars() {
                if character == ' ' {
                    spaces += 1;
                    continue;
                }
                if spaces > 0 {
                    output.push_str(if spaces > 1 { "  " } else { " " });
                    spaces = 0;
                }
                output.push(character);
            }
            output.push('\n');
        }
        output
    }

    fn tail_from<'a>(output: &'a str, start: &str) -> &'a str {
        &output[output.find(start).unwrap()..]
    }

    fn block<'a>(output: &'a str, start: &str, end: &str) -> &'a str {
        let start = output.find(start).unwrap();
        let end = output[start..].find(end).unwrap() + start;
        &output[start..end]
    }

    fn pvc_output(value: Value, pods: &[Pod]) -> String {
        let pvc: PersistentVolumeClaim = from_value(value);
        let mut writer = Writer::new();
        write_pvc(&mut writer, &pvc, pods, NOW_MS);
        writer.finish()
    }

    fn pv_output(value: Value) -> String {
        let pv: PersistentVolume = from_value(value);
        let mut writer = Writer::new();
        write_pv(&mut writer, &pv, NOW_MS);
        writer.finish()
    }

    #[test]
    fn terminating_pending_pvc_prints_conditions_data_source_and_blank_binding_fields() {
        let output = normalize(&pvc_output(
            json!({
                "metadata": {
                    "name": "restore",
                    "namespace": "work",
                    "deletionTimestamp": "2026-07-03T11:50:00Z",
                    "annotations": {"volume.beta.kubernetes.io/storage-class": "legacy"}
                },
                "spec": {
                    "accessModes": ["ReadWriteOnce"],
                    "dataSource": {
                        "apiGroup": "snapshot.storage.k8s.io",
                        "kind": "VolumeSnapshot",
                        "name": "snapshot-1"
                    }
                },
                "status": {
                    "phase": "Pending",
                    "conditions": [{
                        "type": "Resizing",
                        "status": "True",
                        "reason": "ControllerResize",
                        "message": "waiting for resize"
                    }]
                }
            }),
            &[],
        ));

        assert_eq!(
            output,
            concat!(
                "Name:  restore\n",
                "Namespace:  work\n",
                "StorageClass:  legacy\n",
                "Status:  Terminating (lasts 10m)\n",
                "Volume:\n",
                "Labels:  <none>\n",
                "Annotations:  volume.beta.kubernetes.io/storage-class: legacy\n",
                "Finalizers:  []\n",
                "Capacity:\n",
                "Access Modes:\n",
                "DataSource:\n",
                "  APIGroup:  snapshot.storage.k8s.io\n",
                "  Kind:  VolumeSnapshot\n",
                "  Name:  snapshot-1\n",
                "Used By:  <none>\n",
                "Conditions:\n",
                "  Type  Status  LastProbeTime  LastTransitionTime  Reason  Message\n",
                "  ----  ------  -----------------  ------------------  ------  -------\n",
                "  Resizing  True  <unknown>  <unknown>  ControllerResize  waiting for resize\n",
            )
        );
    }

    #[test]
    fn pvc_users_include_mounts_and_pod_owner_uids_in_same_namespace_once_and_sorted() {
        let pvc: PersistentVolumeClaim = from_value(json!({
            "metadata": {
                "name": "claim",
                "namespace": "work",
                "ownerReferences": [{
                    "apiVersion": "v1",
                    "kind": "Pod",
                    "name": "pod-a",
                    "uid": "uid-a"
                }]
            }
        }));
        let pods: Vec<Pod> = [
            json!({"metadata": {"name": "pod-b", "namespace": "work", "uid": "uid-b"},
                "spec": {"volumes": [{"name": "data", "persistentVolumeClaim": {"claimName": "claim"}}]}}),
            json!({"metadata": {"name": "pod-a", "namespace": "work", "uid": "uid-a"}}),
            json!({"metadata": {"name": "pod-b", "namespace": "work", "uid": "uid-b"},
                "spec": {"volumes": [{"name": "again", "persistentVolumeClaim": {"claimName": "claim"}}]}}),
            json!({"metadata": {"name": "pod-c", "namespace": "work"},
                "spec": {"volumes": [{"name": "data", "persistentVolumeClaim": {"claimName": "claim"}}]}}),
            json!({"metadata": {"name": "pod-c", "namespace": "work"},
                "spec": {"volumes": [{"name": "duplicate", "persistentVolumeClaim": {"claimName": "claim"}}]}}),
            json!({"metadata": {"name": "other-namespace", "namespace": "other", "uid": "uid-x"},
                "spec": {"volumes": [{"name": "data", "persistentVolumeClaim": {"claimName": "claim"}}]}}),
        ]
        .into_iter()
        .map(from_value)
        .collect();

        let users = pvc_users(&pvc, &pods);
        assert_eq!(users, ["pod-a", "pod-b", "pod-c"]);
    }

    #[test]
    fn storage_class_precedence_matches_pvc_and_pv_legacy_rules() {
        let metadata: ObjectMeta = from_value(json!({
            "annotations": {"volume.beta.kubernetes.io/storage-class": "legacy"}
        }));
        assert_eq!(pvc_storage_class(&metadata, Some("current")), "legacy");
        assert_eq!(pv_storage_class(&metadata, Some("current")), "legacy");
        assert_eq!(
            pvc_storage_class(&ObjectMeta::default(), Some("current")),
            "current"
        );
        assert_eq!(
            pv_storage_class(&ObjectMeta::default(), Some("current")),
            "current"
        );
    }

    #[test]
    fn core_data_source_omits_api_group_and_precedes_used_by() {
        let output = normalize(&pvc_output(
            json!({
                "metadata": {"name": "copy", "namespace": "work"},
                "spec": {"dataSource": {"kind": "PersistentVolumeClaim", "name": "source"}}
            }),
            &[],
        ));
        assert_eq!(
            tail_from(&output, "DataSource:"),
            "DataSource:\n  Kind:  PersistentVolumeClaim\n  Name:  source\nUsed By:  <none>\n"
        );
    }

    #[test]
    fn pv_node_affinity_handles_absent_empty_and_multiple_requirements() {
        let absent = normalize(&pv_output(
            json!({"metadata": {"name": "absent"}, "spec": {}}),
        ));
        assert_eq!(
            block(&absent, "Node Affinity", "Message:"),
            "Node Affinity:  <none>\n"
        );

        let empty_terms = normalize(&pv_output(json!({
            "metadata": {"name": "empty-terms"},
            "spec": {"nodeAffinity": {"required": {"nodeSelectorTerms": []}}}
        })));
        assert_eq!(
            block(&empty_terms, "Node Affinity", "Message:"),
            "Node Affinity:\n  Required Terms:  <none>\n"
        );

        let empty_expression = normalize(&pv_output(json!({
            "metadata": {"name": "empty-expression"},
            "spec": {"nodeAffinity": {"required": {"nodeSelectorTerms": [{}]}}}
        })));
        assert_eq!(
            block(&empty_expression, "Node Affinity", "Message:"),
            "Node Affinity:\n  Required Terms:\n    Term 0:  <none>\n"
        );

        let multiple = normalize(&pv_output(json!({
            "metadata": {"name": "multiple"},
            "spec": {"nodeAffinity": {"required": {"nodeSelectorTerms": [{
                "matchExpressions": [
                    {"key": "disk", "operator": "In", "values": ["ssd", "nvme"]},
                    {"key": "rack", "operator": "NotIn", "values": ["one"]},
                    {"key": "dedicated", "operator": "Exists"}
                ],
                "matchFields": [{"key": "metadata.name", "operator": "In", "values": ["node-a"]}]
            }]}}}
        })));
        assert_eq!(
            block(&multiple, "Node Affinity", "Message:"),
            "Node Affinity:\n  Required Terms:\n    Term 0:  disk in [ssd, nvme]\n             rack notin [one]\n             dedicated exists\n"
        );
    }

    #[test]
    fn pv_source_variants_and_unknown_fallback_are_rendered() {
        let host_path = normalize(&pv_output(json!({
            "metadata": {"name": "host"},
            "spec": {"hostPath": {"path": "/data"}}
        })));
        assert_eq!(
            tail_from(&host_path, "Source:"),
            "Source:\n  Type:  HostPath (bare host directory volume)\n  Path:  /data\n  HostPathType:  <none>\n"
        );

        let nfs = normalize(&pv_output(json!({
            "metadata": {"name": "nfs"},
            "spec": {"nfs": {"server": "server", "path": "/export", "readOnly": true}}
        })));
        assert_eq!(
            tail_from(&nfs, "Source:"),
            "Source:\n  Type:  NFS (an NFS mount that lasts the lifetime of a pod)\n  Server:  server\n  Path:  /export\n  ReadOnly:  true\n"
        );

        let local = normalize(&pv_output(json!({
            "metadata": {"name": "local"},
            "spec": {"local": {"path": "/mnt/local"}}
        })));
        assert_eq!(
            tail_from(&local, "Source:"),
            "Source:\n  Type:  LocalVolume (a persistent volume backed by local storage on a node)\n  Path:  /mnt/local\n"
        );

        let unknown = normalize(&pv_output(json!({"metadata": {"name": "unknown"}})));
        assert_eq!(tail_from(&unknown, "Source:"), "Source:\n  <unknown>\n");
    }

    #[test]
    fn csi_attributes_are_sorted_and_long_values_are_bounded() {
        let long = "界".repeat(100);
        let output = normalize(&pv_output(json!({
            "metadata": {"name": "csi"},
            "spec": {"csi": {
                "driver": "driver.example",
                "volumeHandle": "handle",
                "volumeAttributes": {"z": long, "a": "first"}
            }}
        })));
        let bounded = format!("z={}...", "界".repeat(84));
        assert_eq!(bounded_attribute("z", &long), bounded);
        assert!(bounded.strip_suffix("...").unwrap().len() <= MAX_VOLUME_ATTRIBUTE_BYTES);
        let lines: Vec<&str> = tail_from(&output, "Source:").lines().collect();
        assert_eq!(lines[6].trim_start(), "VolumeAttributes:  a=first");
        assert_eq!(lines[7].trim_start(), bounded);
    }

    #[test]
    fn terminating_pv_uses_human_duration() {
        let output = normalize(&pv_output(json!({
            "metadata": {
                "name": "deleting",
                "deletionTimestamp": "2026-07-03T09:50:00Z"
            },
            "status": {"phase": "Bound"}
        })));
        assert_eq!(
            output,
            concat!(
                "Name:  deleting\n",
                "Labels:  <none>\n",
                "Annotations:  <none>\n",
                "Finalizers:  []\n",
                "StorageClass:\n",
                "Status:  Terminating (lasts 130m)\n",
                "Claim:\n",
                "Reclaim Policy:\n",
                "Access Modes:\n",
                "Capacity:\n",
                "Node Affinity:  <none>\n",
                "Message:\n",
                "Source:\n",
                "  <unknown>\n",
            )
        );
    }

    #[test]
    fn sparse_storage_statuses_are_blank() {
        assert_eq!(
            normalize(&pvc_output(json!({}), &[])),
            concat!(
                "Name:  <none>\n",
                "Namespace:  <none>\n",
                "StorageClass:\n",
                "Status:\n",
                "Volume:\n",
                "Labels:  <none>\n",
                "Annotations:  <none>\n",
                "Finalizers:  []\n",
                "Capacity:\n",
                "Access Modes:\n",
                "Used By:  <none>\n",
            )
        );
        assert_eq!(
            normalize(&pv_output(json!({}))),
            concat!(
                "Name:  <none>\n",
                "Labels:  <none>\n",
                "Annotations:  <none>\n",
                "Finalizers:  []\n",
                "StorageClass:\n",
                "Status:\n",
                "Claim:\n",
                "Reclaim Policy:\n",
                "Access Modes:\n",
                "Capacity:\n",
                "Node Affinity:  <none>\n",
                "Message:\n",
                "Source:\n",
                "  <unknown>\n",
            )
        );
    }

    #[test]
    fn future_deletion_duration_preserves_signed_truncation() {
        let slightly_future: Time = from_value(json!("2026-07-03T12:00:01.500Z"));
        let sufficiently_future: Time = from_value(json!("2026-07-03T12:00:30Z"));
        assert_eq!(
            phase_with_termination(Some(&slightly_future), Some("Bound"), NOW_MS),
            "Terminating (lasts 0s)"
        );
        assert_eq!(
            phase_with_termination(Some(&sufficiently_future), Some("Bound"), NOW_MS),
            "Terminating (lasts <invalid>)"
        );
        assert_eq!(phase_with_termination(None, None, NOW_MS), "");
    }
}
