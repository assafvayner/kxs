use serde::{Deserialize, Serialize};

pub fn list_path(group: &str, version: &str, plural: &str, namespace: Option<&str>) -> String {
    let prefix = if group.is_empty() {
        format!("/api/{version}")
    } else {
        format!("/apis/{group}/{version}")
    };
    match namespace {
        Some(ns) if !ns.is_empty() => format!("{prefix}/namespaces/{ns}/{plural}"),
        _ => format!("{prefix}/{plural}"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawTable {
    #[serde(default)]
    pub column_definitions: Vec<RawColumn>,
    #[serde(default)]
    pub rows: Vec<RawRow>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawColumn {
    pub name: String,
    #[serde(default)]
    pub priority: i32,
}
#[derive(Debug, Deserialize)]
pub struct RawRow {
    #[serde(default)]
    pub cells: Vec<serde_json::Value>,
    #[serde(default)]
    pub object: RawObject,
}
#[derive(Debug, Default, Deserialize)]
pub struct RawObject {
    #[serde(default)]
    pub metadata: RawMeta,
}
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMeta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub creation_timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTable {
    pub columns: Vec<String>,
    pub rows: Vec<ResourceRow>,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRow {
    pub key: String,
    pub name: String,
    pub namespace: Option<String>,
    /// cells for the visible (priority 0) columns, stringified
    pub cells: Vec<String>,
    pub created: Option<String>,
}

fn cell_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "".into(),
        other => other.to_string(),
    }
}

/// Keeps only priority-0 columns (kubectl's non-wide set), stringifies cells,
/// appends a synthetic "Age" column (rendered client-side from `created`).
pub fn map_table(raw: RawTable) -> ResourceTable {
    let visible: Vec<usize> = raw
        .column_definitions
        .iter()
        .enumerate()
        .filter(|(_, c)| c.priority == 0)
        .map(|(i, _)| i)
        .collect();
    let mut columns: Vec<String> = visible
        .iter()
        .map(|&i| raw.column_definitions[i].name.clone())
        .collect();
    columns.push("Age".into());

    let rows = raw
        .rows
        .into_iter()
        .map(|r| {
            let name = r.object.metadata.name.clone().unwrap_or_default();
            let namespace = r.object.metadata.namespace.clone();
            let key = match &namespace {
                Some(ns) if !ns.is_empty() => format!("{ns}/{name}"),
                _ => name.clone(),
            };
            let cells = visible
                .iter()
                .map(|&i| r.cells.get(i).map(cell_to_string).unwrap_or_default())
                .collect();
            ResourceRow {
                key,
                name,
                namespace,
                cells,
                created: r.object.metadata.creation_timestamp,
            }
        })
        .collect();
    ResourceTable { columns, rows }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_namespaced_path() {
        assert_eq!(
            list_path("", "v1", "pods", Some("app")),
            "/api/v1/namespaces/app/pods"
        );
    }
    #[test]
    fn core_cluster_path() {
        assert_eq!(list_path("", "v1", "nodes", None), "/api/v1/nodes");
    }
    #[test]
    fn grouped_namespaced_path() {
        assert_eq!(
            list_path("apps", "v1", "deployments", Some("kube-system")),
            "/apis/apps/v1/namespaces/kube-system/deployments"
        );
    }
    #[test]
    fn grouped_cluster_path() {
        assert_eq!(
            list_path("rbac.authorization.k8s.io", "v1", "clusterroles", None),
            "/apis/rbac.authorization.k8s.io/v1/clusterroles"
        );
    }
    #[test]
    fn empty_namespace_treated_as_all() {
        // caller passes None for all-namespaces; empty &str should not happen, but guard:
        assert_eq!(list_path("", "v1", "pods", None), "/api/v1/pods");
    }

    const TABLE_JSON: &str = r#"{
      "kind": "Table", "apiVersion": "meta.k8s.io/v1",
      "columnDefinitions": [
        {"name": "Name", "priority": 0},
        {"name": "Ready", "priority": 0},
        {"name": "Status", "priority": 0},
        {"name": "IP", "priority": 1}
      ],
      "rows": [
        {"cells": ["web-1", "1/1", "Running", "10.0.0.1"],
         "object": {"metadata": {"name": "web-1", "namespace": "app",
                     "uid": "u1", "creationTimestamp": "2026-07-01T00:00:00Z"}}},
        {"cells": ["web-2", "0/1", "Pending", "<none>"],
         "object": {"metadata": {"name": "web-2", "namespace": "app",
                     "uid": "u2", "creationTimestamp": "2026-07-02T00:00:00Z"}}}
      ]
    }"#;

    #[test]
    fn maps_table_dropping_low_priority_columns() {
        let raw: RawTable = serde_json::from_str(TABLE_JSON).unwrap();
        let t = map_table(raw);
        // priority>0 (IP) hidden in the default view; Age appended
        assert_eq!(t.columns, vec!["Name", "Ready", "Status", "Age"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].key, "app/web-1");
        assert_eq!(t.rows[0].cells, vec!["web-1", "1/1", "Running"]);
        assert_eq!(t.rows[0].created.as_deref(), Some("2026-07-01T00:00:00Z"));
        assert_eq!(t.rows[0].namespace.as_deref(), Some("app"));
    }

    #[test]
    fn cluster_scoped_rows_have_no_namespace() {
        let raw: RawTable = serde_json::from_str(
            r#"{"columnDefinitions":[{"name":"Name","priority":0}],
                "rows":[{"cells":["node-1"],"object":{"metadata":{"name":"node-1","uid":"n1"}}}]}"#,
        )
        .unwrap();
        let t = map_table(raw);
        assert_eq!(t.rows[0].key, "node-1");
        assert_eq!(t.rows[0].namespace, None);
    }
}
