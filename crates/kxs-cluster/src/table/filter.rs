//! Ported from the filter half of `src/lib/command.ts`.

/// Splits the search bar into a server-side label selector and a client-side
/// name filter: `-l <selector>` optionally followed by a name filter (which may
/// itself be `-r <regex>`). Anything else is a name filter only. The selector
/// ends at the first whitespace, so selectors with spaces (`app in (a, b)`)
/// are not supported. Returns `(labels, name)`.
pub fn split_filter(filter: &str) -> (Option<String>, String) {
    let f = filter.trim();
    // a bare "-l" is a selector still being typed, not a name to match
    if f == "-l" {
        return (None, String::new());
    }
    let Some(rest) = f.strip_prefix("-l ") else {
        return (None, filter.to_string());
    };
    let rest = rest.trim();
    if rest.is_empty() {
        return (None, String::new());
    }
    match rest.find(char::is_whitespace) {
        None => (Some(rest.to_string()), String::new()),
        Some(i) => (
            Some(rest[..i].to_string()),
            rest[i + 1..].trim().to_string(),
        ),
    }
}

/// Compile a filter once; returns a predicate with `match_row`'s semantics.
///
/// k9s grammar: a bare filter is a case-insensitive regex, `!` inverts it,
/// `-f` fuzzy-matches (the query's characters in order, anywhere). `-r` stays
/// as an explicit-regex spelling of the default. An unparseable regex degrades
/// to a substring test rather than matching nothing.
pub fn filter_predicate(filter: &str) -> Box<dyn Fn(&str) -> bool + Send + Sync> {
    let f = filter.trim();
    if f.is_empty() {
        return Box::new(|_| true);
    }
    if let Some(rest) = f.strip_prefix("-f") {
        let needle = rest.trim().to_lowercase();
        if needle.is_empty() {
            return Box::new(|_| true);
        }
        return Box::new(move |name: &str| fuzzy_match(&name.to_lowercase(), &needle));
    }
    let (inverse, rest) = match f.strip_prefix('!') {
        Some(rest) => (true, rest.trim()),
        None => (false, f),
    };
    let pattern = rest.strip_prefix("-r ").map(str::trim).unwrap_or(rest);
    if pattern.is_empty() {
        return Box::new(|_| true);
    }
    match regex::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
    {
        Ok(re) => Box::new(move |name: &str| re.is_match(name) != inverse),
        Err(_) => {
            let needle = pattern.to_lowercase();
            Box::new(move |name: &str| name.to_lowercase().contains(&needle) != inverse)
        }
    }
}

/// Subsequence test: every char of `needle`, in order, somewhere in `hay`.
fn fuzzy_match(hay: &str, needle: &str) -> bool {
    let mut chars = hay.chars();
    needle.chars().all(|c| chars.any(|h| h == c))
}

/// Regex by default; `!` inverts, `-f` fuzzy-matches, `-r` forces regex.
pub fn match_row(name: &str, filter: &str) -> bool {
    filter_predicate(filter)(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_filter_is_a_name_filter() {
        assert_eq!(split_filter("web"), (None, "web".into()));
        assert_eq!(split_filter(""), (None, "".into()));
        assert_eq!(split_filter("-r ^web"), (None, "-r ^web".into()));
    }

    #[test]
    fn extracts_a_label_selector_on_its_own() {
        assert_eq!(
            split_filter("-l app=demo-web"),
            (Some("app=demo-web".into()), "".into())
        );
        assert_eq!(
            split_filter("  -l app=demo-web,tier!=db  "),
            (Some("app=demo-web,tier!=db".into()), "".into())
        );
    }

    #[test]
    fn extracts_a_selector_plus_a_trailing_name_filter() {
        assert_eq!(
            split_filter("-l app=demo-web web-1"),
            (Some("app=demo-web".into()), "web-1".into())
        );
        assert_eq!(
            split_filter("-l app=demo-web -r ^web"),
            (Some("app=demo-web".into()), "-r ^web".into())
        );
    }

    #[test]
    fn bare_l_is_no_filter_at_all() {
        assert_eq!(split_filter("-l"), (None, "".into()));
        assert_eq!(split_filter("-l   "), (None, "".into()));
    }

    #[test]
    fn match_row_substring_case_insensitive() {
        assert!(match_row("web-1", "WEB"));
        assert!(!match_row("api-xyz", "web"));
    }

    #[test]
    fn match_row_regex_with_r_prefix() {
        assert!(match_row("web-1", "-r ^web"));
        assert!(!match_row("api-1", "-r ^web"));
    }

    #[test]
    fn invalid_regex_falls_back_to_substring() {
        assert!(match_row("web[1]", "-r web["));
        assert!(!match_row("api", "-r web["));
    }

    #[test]
    fn bare_filter_is_a_case_insensitive_regex() {
        assert!(match_row("blee-7", "fred|blee"));
        assert!(!match_row("zork-1", "fred|blee"));
        assert!(match_row("web-1", "^WEB"));
    }

    #[test]
    fn bang_inverts_the_match() {
        assert!(!match_row("web-1", "!web"));
        assert!(match_row("api-1", "!web"));
        assert!(match_row("api-1", "!fred|blee"));
    }

    #[test]
    fn dash_f_is_a_fuzzy_subsequence() {
        assert!(match_row("web-server", "-f wbsv"));
        assert!(match_row("web-server", "-f wb"));
        assert!(!match_row("api-gateway", "-f wbsv"));
        // out of order does not match
        assert!(!match_row("web-server", "-f vwb"));
    }

    #[test]
    fn empty_filter_matches_everything() {
        assert!(match_row("anything", ""));
    }
}
