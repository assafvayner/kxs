//! PodRow sorting, ported from `sort.ts::sortPods`.

use super::sort::age_key;
use crate::pods::PodRow;
use std::cmp::Ordering;

use super::sort::{compare_cells, SortDir};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodField {
    Namespace,
    Name,
    Ready,
    Status,
    Restarts,
    Ip,
    Node,
    Age,
}

fn sort_by<F>(rows: &[PodRow], cell: F, dir: SortDir) -> Vec<PodRow>
where
    F: Fn(&PodRow) -> String,
{
    let mut out = rows.to_vec();
    out.sort_by(|x, y| {
        let a = cell(x);
        let b = cell(y);
        // empty cells stay last in both directions (like the TS sortBy)
        if super::is_empty_cell(&a) || super::is_empty_cell(&b) {
            return compare_cells(&a, &b);
        }
        let ord = compare_cells(&a, &b);
        if ord == Ordering::Equal {
            return Ordering::Equal;
        }
        if dir == SortDir::Desc {
            ord.reverse()
        } else {
            ord
        }
    });
    out
}

/// Sort by a number. Missing values stay last in both directions.
fn sort_by_number<F>(rows: &[PodRow], value: F, dir: SortDir) -> Vec<PodRow>
where
    F: Fn(&PodRow) -> Option<i64>,
{
    let mut out = rows.to_vec();
    out.sort_by(|x, y| match (value(x), value(y)) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => {
            if a == b {
                Ordering::Equal
            } else {
                let ord = a.cmp(&b);
                if dir == SortDir::Desc {
                    ord.reverse()
                } else {
                    ord
                }
            }
        }
    });
    out
}

pub fn sort_pods(rows: &[PodRow], field: PodField, dir: SortDir) -> Vec<PodRow> {
    match field {
        PodField::Namespace => sort_by(rows, |p| p.namespace.clone(), dir),
        PodField::Name => sort_by(rows, |p| p.name.clone(), dir),
        PodField::Ready => sort_by(rows, |p| p.ready.clone(), dir),
        PodField::Status => sort_by(rows, |p| p.status.clone(), dir),
        PodField::Restarts => sort_by_number(rows, |p| Some(p.restarts as i64), dir),
        PodField::Ip => sort_by(rows, |p| p.ip.clone().unwrap_or_default(), dir),
        PodField::Node => sort_by(rows, |p| p.node.clone().unwrap_or_default(), dir),
        PodField::Age => sort_by_number(rows, |p| age_key(p.created.as_deref()), dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pod(name: &str, restarts: u32, node: Option<&str>, created: Option<&str>) -> PodRow {
        PodRow {
            key: format!("default/{name}"),
            name: name.into(),
            namespace: "default".into(),
            ready: "1/1".into(),
            status: "Running".into(),
            restarts,
            ip: None,
            node: node.map(String::from),
            created: created.map(String::from),
            cpu_request_millis: None,
            mem_request_mib: None,
        }
    }

    #[test]
    fn sorts_restarts_numerically() {
        let pods = vec![
            pod("web-2", 10, None, None),
            pod("api", 2, None, None),
            pod("web-1", 7, None, None),
        ];
        let asc = sort_pods(&pods, PodField::Restarts, SortDir::Asc);
        let restarts: Vec<u32> = asc.iter().map(|p| p.restarts).collect();
        assert_eq!(restarts, vec![2, 7, 10]);
        let desc = sort_pods(&pods, PodField::Restarts, SortDir::Desc);
        let restarts: Vec<u32> = desc.iter().map(|p| p.restarts).collect();
        assert_eq!(restarts, vec![10, 7, 2]);
    }

    #[test]
    fn sorts_age_by_created_youngest_first() {
        let pods = vec![
            pod("web-2", 0, None, Some("2026-01-01T00:00:00Z")),
            pod("api", 0, None, Some("2026-06-01T00:00:00Z")),
            pod("web-1", 0, None, Some("2026-03-01T00:00:00Z")),
        ];
        let asc = sort_pods(&pods, PodField::Age, SortDir::Asc);
        let names: Vec<&str> = asc.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["api", "web-1", "web-2"]);
    }

    #[test]
    fn sorts_by_name() {
        let pods = vec![
            pod("web-2", 0, None, None),
            pod("api", 0, None, None),
            pod("web-1", 0, None, None),
        ];
        let asc = sort_pods(&pods, PodField::Name, SortDir::Asc);
        let names: Vec<&str> = asc.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["api", "web-1", "web-2"]);
    }

    #[test]
    fn keeps_pods_without_a_node_last_when_sorting_by_node() {
        let pods = vec![
            pod("web-2", 0, Some("n2"), None),
            pod("api", 0, None, None),
            pod("web-1", 0, Some("n1"), None),
        ];
        let desc = sort_pods(&pods, PodField::Node, SortDir::Desc);
        let nodes: Vec<Option<&str>> = desc.iter().map(|p| p.node.as_deref()).collect();
        assert_eq!(nodes, vec![Some("n2"), Some("n1"), None]);
    }

    #[test]
    fn does_not_mutate_the_input() {
        let pods = vec![
            pod("web-2", 10, None, None),
            pod("api", 2, None, None),
            pod("web-1", 7, None, None),
        ];
        let before: Vec<&str> = pods.iter().map(|p| p.name.as_str()).collect();
        let _ = sort_pods(&pods, PodField::Restarts, SortDir::Desc);
        let after: Vec<&str> = pods.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(before, after);
    }
}
