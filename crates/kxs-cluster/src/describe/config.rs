use super::header::write_header;
use super::util::or_none;
use super::writer::Writer;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use serde_json::Value;

pub fn write_configmap(w: &mut Writer, cm: &ConfigMap) {
    write_header(w, &cm.metadata, true);
    w.text(0, "");
    w.text(0, "Data");
    w.text(0, "====");
    if let Some(data) = &cm.data {
        for (k, v) in data {
            w.text(0, &format!("{k}:"));
            w.text(0, "----");
            w.preserved_text(0, v);
            w.text(0, "");
        }
    }
    w.text(0, "");
    w.text(0, "BinaryData");
    w.text(0, "====");
    if let Some(bin) = &cm.binary_data {
        for (k, v) in bin {
            w.text(0, &format!("{k}: {} bytes", v.0.len()));
        }
    }
    w.text(0, "");
}

/// Secret values are never printed, only their byte counts. No Events section.
pub fn write_secret(w: &mut Writer, s: &Secret) {
    write_header(w, &s.metadata, true);
    write_secret_body(
        w,
        s.type_.as_deref(),
        s.data
            .as_ref()
            .into_iter()
            .flatten()
            .map(|(key, value)| (key.as_str(), value.0.len())),
    );
}

/// Fail-closed Secret renderer for objects that do not deserialize as `Secret`.
pub fn write_secret_unstructured(w: &mut Writer, value: &Value) {
    let metadata: ObjectMeta = value
        .get("metadata")
        .cloned()
        .and_then(|metadata| serde_json::from_value(metadata).ok())
        .unwrap_or_default();
    write_header(w, &metadata, true);

    let mut data = Vec::new();
    if let Some(values) = value.get("data").and_then(Value::as_object) {
        data.extend(values.iter().map(|(key, value)| {
            let len = value
                .as_str()
                .and_then(|value| BASE64.decode(value).ok())
                .map_or(0, |value| value.len());
            (key.as_str(), len)
        }));
        data.sort_unstable_by_key(|(key, _)| *key);
    }

    write_secret_body(w, value.get("type").and_then(Value::as_str), data);
}

fn write_secret_body<'a>(
    w: &mut Writer,
    type_: Option<&str>,
    data: impl IntoIterator<Item = (&'a str, usize)>,
) {
    w.text(0, "");
    w.kv(0, "Type", or_none(type_));
    w.text(0, "");
    w.text(0, "Data");
    w.text(0, "====");
    for (key, len) in data {
        w.text(0, &format!("{key}:  {len} bytes"));
    }
    w.text(0, "");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn configmap_escapes_terminal_controls_and_preserves_value_whitespace() {
        let cm = ConfigMap {
            data: Some(BTreeMap::from([(
                "settings".into(),
                "first\u{1b}[31m  \n   \nlast \r".into(),
            )])),
            ..Default::default()
        };
        let mut w = Writer::new();
        write_configmap(&mut w, &cm);
        let output = w.finish();

        assert!(output.contains("first^[[31m  \n   \nlast \\r\n"));
        assert!(!output.contains('\u{1b}'));
        assert!(!output.contains('\r'));
    }
}
