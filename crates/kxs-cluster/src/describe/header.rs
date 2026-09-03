use super::util::{map_lines, or_none, write_list};
use super::writer::Writer;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use std::collections::BTreeMap;

/// kubectl truncates long annotation values at the default verbosity.
const MAX_ANNOTATION_LEN: usize = 256;

/// kubectl's `skipAnnotations`: this annotation carries a full copy of the
/// object, including data the describer deliberately withholds, and is never
/// printed.
const LAST_APPLIED_CONFIGURATION: &str = "kubectl.kubernetes.io/last-applied-configuration";

/// Name / Namespace (when namespaced) / Labels / Annotations.
pub fn write_header(w: &mut Writer, meta: &ObjectMeta, namespaced: bool) {
    w.kv(0, "Name", or_none(meta.name.as_deref()));
    if namespaced {
        w.kv(0, "Namespace", or_none(meta.namespace.as_deref()));
    }
    write_labels_annotations(w, meta);
}

pub fn write_labels_annotations(w: &mut Writer, meta: &ObjectMeta) {
    write_list(w, 0, "Labels", &map_lines(meta.labels.as_ref()));
    write_list(
        w,
        0,
        "Annotations",
        &annotation_lines(meta.annotations.as_ref()),
    );
}

/// `key: value` lines, values truncated to 256 chars with `...`.
pub fn annotation_lines(m: Option<&BTreeMap<String, String>>) -> Vec<String> {
    m.into_iter()
        .flatten()
        .filter(|(k, _)| k.as_str() != LAST_APPLIED_CONFIGURATION)
        .map(|(k, v)| {
            let v = if v.chars().count() > MAX_ANNOTATION_LEN {
                format!(
                    "{}...",
                    v.chars().take(MAX_ANNOTATION_LEN).collect::<String>()
                )
            } else {
                v.clone()
            };
            format!("{k}: {v}")
        })
        .collect()
}

/// `Controlled By:  Kind/name` for the controller owner reference, if any.
pub fn write_controlled_by(w: &mut Writer, meta: &ObjectMeta) {
    if let Some(owner) = meta
        .owner_references
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .find(|r| r.controller == Some(true))
    {
        w.kv(0, "Controlled By", format!("{}/{}", owner.kind, owner.name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;

    #[test]
    fn header_prints_name_namespace_labels_annotations() {
        let meta = ObjectMeta {
            name: Some("web".into()),
            namespace: Some("default".into()),
            labels: Some(
                [("app".to_string(), "web".to_string())]
                    .into_iter()
                    .collect(),
            ),
            annotations: Some([("k".to_string(), "v".to_string())].into_iter().collect()),
            ..Default::default()
        };
        let mut w = Writer::new();
        write_header(&mut w, &meta, true);
        assert_eq!(
            w.finish(),
            "Name:         web\nNamespace:    default\nLabels:       app=web\nAnnotations:  k: v\n"
        );
    }

    #[test]
    fn cluster_scoped_header_omits_namespace_and_prints_none() {
        let meta = ObjectMeta {
            name: Some("node-a".into()),
            ..Default::default()
        };
        let mut w = Writer::new();
        write_header(&mut w, &meta, false);
        assert_eq!(
            w.finish(),
            "Name:         node-a\nLabels:       <none>\nAnnotations:  <none>\n"
        );
    }

    #[test]
    fn long_annotations_are_truncated() {
        let long = "x".repeat(300);
        let lines = annotation_lines(Some(&[("k".to_string(), long)].into_iter().collect()));
        assert_eq!(lines[0].len(), "k: ".len() + 256 + 3);
        assert!(lines[0].ends_with("..."));
    }

    #[test]
    fn last_applied_configuration_is_never_printed() {
        let meta = ObjectMeta {
            annotations: Some(
                [(
                    "kubectl.kubernetes.io/last-applied-configuration".to_string(),
                    "{\"data\":{\"password\":\"marker\"}}".to_string(),
                )]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        };
        assert!(annotation_lines(meta.annotations.as_ref()).is_empty());
        let mut w = Writer::new();
        write_labels_annotations(&mut w, &meta);
        let output = w.finish();
        assert!(!output.contains("marker"));
        assert!(output.contains("Annotations:  <none>\n"));
    }

    #[test]
    fn controlled_by_uses_the_controller_owner() {
        let meta = ObjectMeta {
            owner_references: Some(vec![
                OwnerReference {
                    kind: "Foo".into(),
                    name: "x".into(),
                    controller: None,
                    ..Default::default()
                },
                OwnerReference {
                    kind: "ReplicaSet".into(),
                    name: "web-7d9f".into(),
                    controller: Some(true),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };
        let mut w = Writer::new();
        write_controlled_by(&mut w, &meta);
        assert_eq!(w.finish(), "Controlled By:  ReplicaSet/web-7d9f\n");
        let mut w = Writer::new();
        write_controlled_by(&mut w, &ObjectMeta::default());
        assert_eq!(w.finish(), "");
    }
}
