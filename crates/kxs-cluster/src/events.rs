//! Event helpers, ported from `src/lib/events.ts` (server-side Event Table
//! rows are unordered).

use k8s_openapi::chrono::DateTime;
use kxs_core::format::parse_human_duration;

use crate::resources::ResourceRow;

/// Index of a Table column by (case-insensitive) name, or -1.
pub fn column_index(columns: &[String], name: &str) -> i32 {
    let want = name.trim().to_lowercase();
    columns
        .iter()
        .position(|c| c.trim().to_lowercase() == want)
        .map(|i| i as i32)
        .unwrap_or(-1)
}

/// Absolute epoch ms to order an event by: its creationTimestamp when parsable,
/// else `now_ms` minus the Last Seen duration. Unusable rows sort last
/// (`i64::MIN` stands in for the TS `Number.NEGATIVE_INFINITY`).
pub fn event_time_ms(
    created: Option<&str>,
    cells: &[String],
    last_seen_index: i32,
    now_ms: i64,
) -> i64 {
    if let Some(created) = created {
        if let Ok(t) = DateTime::parse_from_rfc3339(created) {
            return t.timestamp_millis();
        }
    }
    if last_seen_index >= 0 {
        if let Some(cell) = cells.get(last_seen_index as usize) {
            if let Some(secs) = parse_human_duration(cell) {
                return now_ms - secs * 1000;
            }
        }
    }
    i64::MIN
}

/// Newest first. Stable, so equal timestamps keep the server's relative order.
pub fn sort_events_newest_first(
    rows: &[ResourceRow],
    last_seen_index: i32,
    now_ms: i64,
) -> Vec<ResourceRow> {
    let mut keyed: Vec<(i64, ResourceRow)> = rows
        .iter()
        .map(|r| {
            (
                event_time_ms(r.created.as_deref(), &r.cells, last_seen_index, now_ms),
                r.clone(),
            )
        })
        .collect();
    keyed.sort_by_key(|(t, _)| std::cmp::Reverse(*t)); // stable: equal keys keep relative order
    keyed.into_iter().map(|(_, r)| r).collect()
}

/// Text the `/` filter matches against: reason + object + message.
pub fn event_filter_text(row: &ResourceRow, indices: &[i32]) -> String {
    indices
        .iter()
        .filter(|&&i| i >= 0)
        .map(|&i| row.cells.get(i as usize).cloned().unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Visual weight of an event type: Warning stands out, Normal recedes,
/// anything unexpected is flagged.
pub fn event_type_weight(type_: &str) -> Weight {
    match type_.trim().to_lowercase().as_str() {
        "warning" => Weight::Bad,
        "normal" => Weight::Dim,
        _ => Weight::Warn,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    None,
    Dim,
    Warn,
    Bad,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(created: Option<&str>, cells: Vec<&str>) -> ResourceRow {
        ResourceRow {
            key: String::new(),
            name: String::new(),
            namespace: None,
            cells: cells.into_iter().map(String::from).collect(),
            created: created.map(String::from),
        }
    }

    #[test]
    fn column_index_finds_case_insensitively() {
        let columns = ["Last Seen", "Type", "Reason", "Object", "Message", "Age"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        assert_eq!(column_index(&columns, "last seen"), 0);
        assert_eq!(column_index(&columns, "MESSAGE"), 4);
        assert_eq!(column_index(&columns, "subobject"), -1);
    }

    #[test]
    fn event_time_prefers_creation_timestamp() {
        let now = 1_767_268_800_000; // 2026-01-01T12:00:00Z
        let r = row(Some("2026-01-01T11:00:00Z"), vec!["9h"]);
        assert_eq!(
            event_time_ms(Some("2026-01-01T11:00:00Z"), &r.cells, 0, now),
            1_767_265_200_000
        );
    }

    #[test]
    fn event_time_falls_back_to_last_seen_cell() {
        let now = 1_767_268_800_000; // 2026-01-01T12:00:00Z
        assert_eq!(
            event_time_ms(None, &["10m".to_string()], 0, now),
            now - 600_000
        );
        assert_eq!(
            event_time_ms(Some("not-a-date"), &["10m".to_string()], 0, now),
            now - 600_000
        );
    }

    #[test]
    fn event_time_sorts_last_when_unusable() {
        let now = 1_767_268_800_000; // 2026-01-01T12:00:00Z
        assert_eq!(
            event_time_ms(None, &["<unknown>".to_string()], 0, now),
            i64::MIN
        );
        assert_eq!(event_time_ms(None, &["5m".to_string()], -1, now), i64::MIN);
    }

    #[test]
    fn sort_events_newest_first_across_both_sources() {
        let now = 1_767_268_800_000; // 2026-01-01T12:00:00Z
        let rows = vec![
            row(None, vec!["<unknown>", "oldest-unusable"]),
            row(Some("2026-01-01T09:00:00Z"), vec!["3h", "three-hours"]),
            row(None, vec!["30s", "thirty-seconds"]),
            row(Some("2026-01-01T11:30:00Z"), vec!["30m", "thirty-minutes"]),
        ];
        let sorted = sort_events_newest_first(&rows, 0, now);
        let names: Vec<&str> = sorted.iter().map(|r| r.cells[1].as_str()).collect();
        assert_eq!(
            names,
            vec![
                "thirty-seconds",
                "thirty-minutes",
                "three-hours",
                "oldest-unusable"
            ]
        );
    }

    #[test]
    fn sort_events_is_stable() {
        let now = 1_767_268_800_000; // 2026-01-01T12:00:00Z
        let rows = vec![
            row(None, vec!["1m", "a"]),
            row(None, vec!["1m", "b"]),
            row(None, vec!["1m", "c"]),
        ];
        let sorted = sort_events_newest_first(&rows, 0, now);
        let names: Vec<&str> = sorted.iter().map(|r| r.cells[1].as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn event_filter_text_joins_requested_cells() {
        let r = row(
            None,
            vec![
                "5m",
                "Warning",
                "BackOff",
                "pod/web-1",
                "Back-off pulling image",
            ],
        );
        assert_eq!(
            event_filter_text(&r, &[2, 3, 4]),
            "BackOff pod/web-1 Back-off pulling image"
        );
        assert_eq!(
            event_filter_text(&r, &[2, -1, 4]),
            "BackOff Back-off pulling image"
        );
    }

    #[test]
    fn event_type_weights() {
        assert_eq!(event_type_weight("Warning"), Weight::Bad);
        assert_eq!(event_type_weight("normal"), Weight::Dim);
        assert_eq!(event_type_weight("Weird"), Weight::Warn);
        assert_eq!(event_type_weight(""), Weight::Warn);
    }
}
