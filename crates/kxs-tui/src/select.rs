//! Selection movement, ported from `clusterKeys.ts::moveSelection` (the DOM
//! parts of that module stay in the desktop app).

/// Next selected key after moving by `delta` in `keys` (visible row order).
/// No selection (or a stale one) starts from the top/bottom edge; empty list → `None`.
pub fn move_selection(keys: &[String], selected: Option<&str>, delta: isize) -> Option<String> {
    if keys.is_empty() {
        return None;
    }
    let i = match selected.and_then(|s| keys.iter().position(|k| k == s)) {
        Some(i) => i as isize,
        None => {
            return Some(if delta > 0 {
                keys[0].clone()
            } else {
                keys[keys.len() - 1].clone()
            })
        }
    };
    let next = (i + delta).clamp(0, keys.len() as isize - 1);
    Some(keys[next as usize].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> Vec<String> {
        vec!["a", "b", "c", "d"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    #[test]
    fn moves_within_bounds() {
        let k = keys();
        assert_eq!(move_selection(&k, Some("b"), 1).as_deref(), Some("c"));
        assert_eq!(move_selection(&k, Some("b"), -1).as_deref(), Some("a"));
        assert_eq!(move_selection(&k, Some("d"), 5).as_deref(), Some("d"));
        assert_eq!(move_selection(&k, Some("a"), -5).as_deref(), Some("a"));
    }

    #[test]
    fn no_selection_starts_at_edge() {
        let k = keys();
        assert_eq!(move_selection(&k, None, 1).as_deref(), Some("a"));
        assert_eq!(move_selection(&k, None, -1).as_deref(), Some("d"));
    }

    #[test]
    fn stale_selection_starts_at_edge() {
        let k = keys();
        assert_eq!(move_selection(&k, Some("zz"), 1).as_deref(), Some("a"));
    }

    #[test]
    fn empty_list_is_none() {
        assert_eq!(move_selection(&[], Some("a"), 1), None);
    }
}
