//! Command-bar kind resolution, ported from `src/lib/command.ts`.

use crate::discovery::ResourceKind;
use std::collections::HashSet;

pub fn resolve_kind(kinds: &[ResourceKind], query: &str) -> Option<ResourceKind> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    kinds
        .iter()
        .find(|k| k.aliases.contains(&q) || k.kind.to_lowercase() == q || k.plural == q)
        .cloned()
}

/// Kinds visible in the picker: all when unprobed; else cluster-scoped + present namespaced.
pub fn visible_kinds<'a>(
    kinds: &'a [ResourceKind],
    present: Option<&HashSet<String>>,
) -> Vec<&'a ResourceKind> {
    match present {
        None => kinds.iter().collect(),
        Some(present) => kinds
            .iter()
            .filter(|k| !k.namespaced || present.contains(&format!("{}/{}", k.group, k.kind)))
            .collect(),
    }
}

pub fn fuzzy_kinds(kinds: &[ResourceKind], query: &str) -> Vec<ResourceKind> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return kinds.to_vec();
    }
    let score = |k: &ResourceKind| -> u8 {
        if k.aliases.contains(&q) {
            return 0;
        }
        if k.kind.to_lowercase().starts_with(&q) || k.plural.starts_with(&q) {
            return 1;
        }
        if k.kind.to_lowercase().contains(&q) || k.aliases.iter().any(|a| a.contains(&q)) {
            return 2;
        }
        99
    };
    let mut scored: Vec<(u8, &ResourceKind)> = kinds
        .iter()
        .map(|k| (score(k), k))
        .filter(|(s, _)| *s < 99)
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.kind.cmp(&b.1.kind)));
    scored.into_iter().map(|(_, k)| k.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds() -> Vec<ResourceKind> {
        vec![
            ResourceKind {
                group: "".into(),
                version: "v1".into(),
                kind: "Pod".into(),
                plural: "pods".into(),
                namespaced: true,
                aliases: vec!["po".into(), "pod".into(), "pods".into()],
            },
            ResourceKind {
                group: "apps".into(),
                version: "v1".into(),
                kind: "Deployment".into(),
                plural: "deployments".into(),
                namespaced: true,
                aliases: vec!["deploy".into(), "deployment".into(), "deployments".into()],
            },
            ResourceKind {
                group: "".into(),
                version: "v1".into(),
                kind: "Service".into(),
                plural: "services".into(),
                namespaced: true,
                aliases: vec!["service".into(), "services".into(), "svc".into()],
            },
        ]
    }

    #[test]
    fn resolve_matches_alias_kind_or_plural_case_insensitively() {
        let kinds = kinds();
        assert_eq!(resolve_kind(&kinds, "po").unwrap().kind, "Pod");
        assert_eq!(resolve_kind(&kinds, "PODS").unwrap().kind, "Pod");
        assert_eq!(resolve_kind(&kinds, "svc").unwrap().kind, "Service");
        assert_eq!(resolve_kind(&kinds, "deploy").unwrap().kind, "Deployment");
    }

    #[test]
    fn resolve_unknown_is_none() {
        assert!(resolve_kind(&kinds(), "nope").is_none());
    }

    #[test]
    fn fuzzy_ranks_exact_alias_first_then_substring() {
        let r = fuzzy_kinds(&kinds(), "dep");
        assert_eq!(r[0].kind, "Deployment");
    }

    #[test]
    fn fuzzy_empty_query_returns_all() {
        assert_eq!(fuzzy_kinds(&kinds(), "").len(), 3);
    }

    fn node() -> ResourceKind {
        ResourceKind {
            group: "".into(),
            version: "v1".into(),
            kind: "Node".into(),
            plural: "nodes".into(),
            namespaced: false,
            aliases: vec!["no".into()],
        }
    }

    #[test]
    fn visible_returns_all_when_not_probed() {
        let mut all = kinds();
        all.push(node());
        let r = visible_kinds(&all, None);
        assert_eq!(r.len(), 4);
    }

    #[test]
    fn visible_keeps_only_present_namespaced_kinds() {
        let present: HashSet<String> = ["/Pod".to_string()].into();
        let ks = kinds();
        let r = visible_kinds(&ks, Some(&present));
        let names: Vec<_> = r.iter().map(|k| k.kind.as_str()).collect();
        assert_eq!(names, vec!["Pod"]);
    }

    #[test]
    fn visible_always_keeps_cluster_scoped_kinds() {
        let empty: HashSet<String> = HashSet::new();
        let ks = vec![node()];
        let r = visible_kinds(&ks, Some(&empty));
        let names: Vec<_> = r.iter().map(|k| k.kind.as_str()).collect();
        assert_eq!(names, vec!["Node"]);
    }
}
