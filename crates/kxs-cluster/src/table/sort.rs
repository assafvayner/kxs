use crate::quantity::{parse_quantity, split_number};
use crate::resources::ResourceRow;
use k8s_openapi::chrono::DateTime;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub key: usize,
    pub dir: SortDir,
}

/// Renderings of "no value" used by the apiserver's Table printer and by us.
const EMPTY_CELLS: [&str; 6] = ["", "-", "—", "<none>", "<unknown>", "<invalid>"];

pub fn is_empty_cell(v: &str) -> bool {
    EMPTY_CELLS.contains(&v.trim())
}

/// The whole cell as a k8s quantity ("3", "250m", "128Mi"), else `None`.
/// `parse_quantity` also rejects trailing junk ("90s", "2/2"), matching the
/// TS whole-cell regex.
fn quantity(v: &str) -> Option<f64> {
    parse_quantity(v.trim())
}

/// Leading number plus whatever follows it ("3 (5m ago)" → (3, " (5m ago)")));
/// the number is the longest f64-parseable prefix, matching the TS regex
/// (`10.0.0.1` → 10.0 + ".0.0.1").
fn leading(v: &str) -> Option<(f64, &str)> {
    split_number(v.trim())
}

fn cmp_case_insensitive(a: &str, b: &str) -> Ordering {
    let al = a.to_lowercase();
    let bl = b.to_lowercase();
    al.cmp(&bl)
}

/// Category rank so mixed columns still form a total order: quantities, then
/// leading-number cells, then plain strings, then empties.
fn rank(v: &str) -> u8 {
    if is_empty_cell(v) {
        3
    } else if quantity(v).is_some() {
        0
    } else if leading(v).is_some() {
        1
    } else {
        2
    }
}

/// Numeric-aware cell order: quantities and leading numbers compare numerically
/// ("2" < "10", "250m" < "1", "128Mi" < "1Gi"), empty cells sort last, anything
/// else compares case-insensitively.
pub fn compare_cells(a: &str, b: &str) -> Ordering {
    let (ra, rb) = (rank(a), rank(b));
    if ra != rb {
        return ra.cmp(&rb);
    }
    match ra {
        3 => Ordering::Equal,
        0 => quantity(a)
            .partial_cmp(&quantity(b))
            .unwrap_or(Ordering::Equal),
        1 => {
            let (la, rest_a) = leading(a).expect("rank 1");
            let (lb, rest_b) = leading(b).expect("rank 1");
            match la.partial_cmp(&lb) {
                Some(Ordering::Equal) | None => cmp_case_insensitive(rest_a, rest_b),
                Some(ord) => ord,
            }
        }
        _ => cmp_case_insensitive(a.trim(), b.trim()),
    }
}

/// Sort key for an Age column: ascending *age* means youngest first, so the key
/// is the negated creation instant. Absent/unparseable → `None` (sorts last).
pub fn age_key(created: Option<&str>) -> Option<i64> {
    DateTime::parse_from_rfc3339(created?)
        .ok()
        .map(|t| -t.timestamp_millis())
}

/// Header click cycle: none → asc → desc → none for `key`.
pub fn cycle_sort(cur: Option<Sort>, key: usize) -> Option<Sort> {
    match cur {
        Some(s) if s.key == key => match s.dir {
            SortDir::Asc => Some(Sort {
                key,
                dir: SortDir::Desc,
            }),
            SortDir::Desc => None,
        },
        _ => Some(Sort {
            key,
            dir: SortDir::Asc,
        }),
    }
}

pub fn sort_indicator(cur: Option<Sort>, key: usize) -> &'static str {
    match cur {
        Some(s) if s.key == key => match s.dir {
            SortDir::Asc => "▲",
            SortDir::Desc => "▼",
        },
        _ => "",
    }
}

/// Server-side table rows by column index. The trailing synthetic Age column
/// (index >= cells.len()) sorts by the creation timestamp, not its rendering.
pub fn sort_rows(rows: &[ResourceRow], col: usize, dir: SortDir) -> Vec<ResourceRow> {
    let cell_count = rows.first().map_or(0, |r| r.cells.len());
    let mut out = rows.to_vec();
    if col >= cell_count {
        out.sort_by(
            |x, y| match (age_key(x.created.as_deref()), age_key(y.created.as_deref())) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => {
                    let ord = a.cmp(&b);
                    if dir == SortDir::Desc {
                        ord.reverse()
                    } else {
                        ord
                    }
                }
            },
        );
    } else {
        out.sort_by(|x, y| {
            let a = x.cells.get(col).map_or("", String::as_str);
            let b = y.cells.get(col).map_or("", String::as_str);
            if is_empty_cell(a) || is_empty_cell(b) {
                return compare_cells(a, b);
            }
            let ord = compare_cells(a, b);
            if dir == SortDir::Desc {
                ord.reverse()
            } else {
                ord
            }
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut cells: Vec<&str>) -> Vec<&str> {
        cells.sort_by(|a, b| compare_cells(a, b));
        cells
    }

    #[test]
    fn plain_numbers_compare_numerically() {
        assert_eq!(sorted(vec!["10", "2", "1"]), vec!["1", "2", "10"]);
        assert_eq!(compare_cells("2", "10"), Ordering::Less);
    }

    #[test]
    fn memory_quantities_across_suffixes() {
        assert_eq!(
            sorted(vec!["1Gi", "128Mi", "512Mi", "2Ki"]),
            vec!["2Ki", "128Mi", "512Mi", "1Gi"]
        );
    }

    #[test]
    fn cpu_quantities_with_milli_suffix() {
        assert_eq!(
            sorted(vec!["1", "250m", "2500m"]),
            vec!["250m", "1", "2500m"]
        );
    }

    #[test]
    fn emptyish_cells_sort_last_in_both_orders() {
        for empty in ["", "  ", "-", "—", "<none>", "<unknown>"] {
            assert_eq!(compare_cells(empty, "web"), Ordering::Greater);
            assert_eq!(compare_cells("web", empty), Ordering::Less);
        }
        assert_eq!(compare_cells("", "<none>"), Ordering::Equal);
    }

    #[test]
    fn falls_back_to_case_insensitive_string_compare() {
        assert_eq!(compare_cells("Web", "api"), Ordering::Greater);
        assert_eq!(compare_cells("web", "WEB"), Ordering::Equal);
        assert_eq!(
            sorted(vec!["Running", "completed", "Pending"]),
            vec!["completed", "Pending", "Running"]
        );
    }

    #[test]
    fn leading_number_with_trailing_remainder() {
        assert_eq!(
            sorted(vec!["10 (5m ago)", "2 (1h ago)"]),
            vec!["2 (1h ago)", "10 (5m ago)"]
        );
        assert_eq!(
            sorted(vec!["0/1", "2/2", "10/10"]),
            vec!["0/1", "2/2", "10/10"]
        );
    }

    #[test]
    fn leading_number_ties_break_on_remainder() {
        assert_eq!(compare_cells("2/2", "2/3"), Ordering::Less);
    }

    #[test]
    fn dotted_values_are_not_quantities() {
        assert_eq!(
            sorted(vec!["10.0.0.1", "9.0.0.1"]),
            vec!["9.0.0.1", "10.0.0.1"]
        );
    }

    #[test]
    fn compare_cells_is_transitive_across_categories() {
        // previously a 3-cycle: 512Mi < 1Gi, 2/2 < 512Mi, 1Gi < 2/2
        assert_eq!(
            sorted(vec!["2/2", "1Gi", "512Mi"]),
            vec!["512Mi", "1Gi", "2/2"]
        );
        assert_eq!(
            sorted(vec!["web", "2/2", "3", ""]),
            vec!["3", "2/2", "web", ""]
        );
    }

    #[test]
    fn unknown_suffix_is_a_plain_string_tail() {
        assert_eq!(sorted(vec!["90s", "3s"]), vec!["3s", "90s"]);
    }

    #[test]
    fn empty_cell_recognition() {
        assert!(is_empty_cell("<none>"));
        assert!(is_empty_cell(" "));
        assert!(!is_empty_cell("0"));
    }

    #[test]
    fn sort_by_keeps_empties_last_in_both_directions() {
        let rows = ["b", "", "a", "c"];
        let mut rows: Vec<(usize, &str)> = rows.iter().copied().enumerate().collect();
        rows.sort_by(|x, y| {
            let ord = compare_cells(x.1, y.1);
            if x.1.is_empty() || y.1.is_empty() {
                return ord;
            }
            ord.reverse() // desc
        });
        assert_eq!(
            rows.iter().map(|r| r.1).collect::<Vec<_>>(),
            vec!["c", "b", "a", ""]
        );
    }

    #[test]
    fn age_key_orders_youngest_first() {
        let older = age_key(Some("2026-01-01T00:00:00Z")).unwrap();
        let newer = age_key(Some("2026-06-01T00:00:00Z")).unwrap();
        assert!(newer < older);
    }

    #[test]
    fn age_key_none_for_missing_or_unparseable() {
        assert_eq!(age_key(None), None);
        assert_eq!(age_key(Some("not-a-date")), None);
    }

    #[test]
    fn cycle_sort_cycles_on_the_same_key() {
        let asc = cycle_sort(None, 1);
        assert_eq!(
            asc,
            Some(Sort {
                key: 1,
                dir: SortDir::Asc
            })
        );
        let desc = cycle_sort(asc, 1);
        assert_eq!(
            desc,
            Some(Sort {
                key: 1,
                dir: SortDir::Desc
            })
        );
        assert_eq!(cycle_sort(desc, 1), None);
    }

    #[test]
    fn cycle_sort_restarts_on_a_different_key() {
        assert_eq!(
            cycle_sort(
                Some(Sort {
                    key: 1,
                    dir: SortDir::Desc
                }),
                2
            ),
            Some(Sort {
                key: 2,
                dir: SortDir::Asc
            })
        );
    }

    #[test]
    fn sort_indicator_marks_only_the_sorted_key() {
        let s = Some(Sort {
            key: 0,
            dir: SortDir::Asc,
        });
        assert_eq!(sort_indicator(s, 0), "▲");
        assert_eq!(
            sort_indicator(
                Some(Sort {
                    key: 0,
                    dir: SortDir::Desc
                }),
                0
            ),
            "▼"
        );
        assert_eq!(sort_indicator(s, 1), "");
        assert_eq!(sort_indicator(None, 0), "");
    }

    fn row(name: &str, cells: Vec<&str>, created: &str) -> ResourceRow {
        ResourceRow {
            key: format!("default/{name}"),
            name: name.into(),
            namespace: Some("default".into()),
            cells: cells.into_iter().map(String::from).collect(),
            created: Some(created.into()),
        }
    }

    #[test]
    fn sort_rows_by_data_column() {
        let rows = [
            row("b", vec!["b", "10"], "2026-01-01T00:00:00Z"),
            row("a", vec!["a", "2"], "2026-06-01T00:00:00Z"),
            row("c", vec!["c", ""], "2026-03-01T00:00:00Z"),
        ];
        let asc = sort_rows(&rows, 1, SortDir::Asc);
        assert_eq!(
            asc.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        let desc = sort_rows(&rows, 1, SortDir::Desc);
        assert_eq!(
            desc.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["b", "a", "c"]
        );
    }

    #[test]
    fn sort_rows_age_uses_created_not_the_rendered_string() {
        let rows = [
            row("b", vec!["b", "10"], "2026-01-01T00:00:00Z"),
            row("a", vec!["a", "2"], "2026-06-01T00:00:00Z"),
            row("c", vec!["c", ""], "2026-03-01T00:00:00Z"),
        ];
        let asc = sort_rows(&rows, 2, SortDir::Asc);
        assert_eq!(
            asc.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["a", "c", "b"]
        );
        let desc = sort_rows(&rows, 2, SortDir::Desc);
        assert_eq!(
            desc.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["b", "c", "a"]
        );
    }

    #[test]
    fn sort_rows_out_of_range_column_is_age() {
        let rows = [
            row("b", vec!["b", "10"], "2026-01-01T00:00:00Z"),
            row("a", vec!["a", "2"], "2026-06-01T00:00:00Z"),
            row("c", vec!["c", ""], "2026-03-01T00:00:00Z"),
        ];
        let asc = sort_rows(&rows, 9, SortDir::Asc);
        assert_eq!(
            asc.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["a", "c", "b"]
        );
    }

    #[test]
    fn sort_rows_empty_set() {
        assert!(sort_rows(&[], 0, SortDir::Asc).is_empty());
    }
}
