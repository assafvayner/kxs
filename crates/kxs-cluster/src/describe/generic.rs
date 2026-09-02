use super::header::write_header;
use super::util::{or_none, title_case};
use super::writer::Writer;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use serde_json::{Map, Value};

/// Fallback for kinds without a typed describer (CRDs included): header,
/// API Version, Kind, then every top-level field as an indented key tree.
pub fn write(w: &mut Writer, value: &Value, namespaced: bool) {
    let meta: ObjectMeta = value
        .get("metadata")
        .cloned()
        .and_then(|m| serde_json::from_value(m).ok())
        .unwrap_or_default();
    write_header(w, &meta, namespaced);
    w.kv(
        0,
        "API Version",
        or_none(value.get("apiVersion").and_then(Value::as_str)),
    );
    w.kv(
        0,
        "Kind",
        or_none(value.get("kind").and_then(Value::as_str)),
    );
    let Some(obj) = value.as_object() else { return };
    let mut keys: Vec<&String> = obj
        .keys()
        .filter(|k| !matches!(k.as_str(), "apiVersion" | "kind"))
        .collect();
    keys.sort();
    for k in keys {
        if k == "metadata" {
            let Some(m) = obj[k].as_object() else {
                continue;
            };
            let trimmed: Map<String, Value> = m
                .iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.as_str(),
                        "name" | "namespace" | "labels" | "annotations" | "managedFields"
                    )
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            write_tree(w, 0, "Metadata", &Value::Object(trimmed));
        } else {
            write_tree(w, 0, &title_case(k), &obj[k]);
        }
    }
}

pub fn write_tree(w: &mut Writer, level: usize, key: &str, v: &Value) {
    match v {
        Value::Object(m) => {
            w.section(level, key);
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            for k in keys {
                write_tree(w, level + 1, &title_case(k), &m[k]);
            }
        }
        Value::Array(items) => {
            w.section(level, key);
            for item in items {
                match item {
                    Value::Object(m) => {
                        let mut keys: Vec<&String> = m.keys().collect();
                        keys.sort();
                        for k in keys {
                            write_tree(w, level + 1, &title_case(k), &m[k]);
                        }
                    }
                    other => w.text(level + 1, &scalar(other)),
                }
            }
        }
        other => w.kv(level, key, scalar(other)),
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "<nil>".into(),
        other => other.to_string(),
    }
}
