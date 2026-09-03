use super::writer::Writer;
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, Time};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kxs_core::format::human_duration;
use std::collections::BTreeMap;

pub const NONE: &str = "<none>";
pub const UNSET: &str = "<unset>";
pub const UNKNOWN: &str = "<unknown>";

/// kubectl prints timestamps as RFC1123Z: `Wed, 01 Jul 2026 00:00:00 +0000`.
pub fn rfc1123z(t: &Time) -> String {
    t.0.format("%a, %d %b %Y %H:%M:%S %z").to_string()
}

/// kubectl's status for an object with a deletion timestamp.
pub fn terminating_status(deletion_timestamp: &Time, now_ms: i64) -> String {
    let seconds = (now_ms - deletion_timestamp.0.timestamp_millis()) / 1000;
    format!("Terminating (lasts {})", human_duration(seconds))
}

/// kubectl's `labels.FormatLabels`: sorted `k=v` pairs on one comma-joined line.
pub fn format_labels(m: Option<&BTreeMap<String, String>>) -> String {
    let pairs = map_lines(m);
    if pairs.is_empty() {
        NONE.to_string()
    } else {
        pairs.join(",")
    }
}

pub fn or_none(s: Option<&str>) -> &str {
    match s {
        Some(v) if !v.is_empty() => v,
        _ => NONE,
    }
}

pub fn bool_title(b: bool) -> &'static str {
    if b {
        "True"
    } else {
        "False"
    }
}

pub fn int_or_string(v: &IntOrString) -> String {
    match v {
        IntOrString::Int(i) => i.to_string(),
        IntOrString::String(s) => s.clone(),
    }
}

/// `k=v` lines, sorted by key (BTreeMap order).
pub fn map_lines(m: Option<&BTreeMap<String, String>>) -> Vec<String> {
    m.map(|m| m.iter().map(|(k, v)| format!("{k}={v}")).collect())
        .unwrap_or_default()
}

/// `Key:  first` then continuation lines for the rest; `<none>` when empty.
pub fn write_list(w: &mut Writer, level: usize, key: &str, items: &[String]) {
    match items.split_first() {
        None => w.kv(level, key, NONE),
        Some((first, rest)) => {
            w.kv(level, key, first);
            for r in rest {
                w.cont(level, r);
            }
        }
    }
}

/// `Key:` section with one `name:  quantity` child per entry. Omitted when
/// empty, matching kubectl's Limits/Requests handling.
pub fn write_quantities(
    w: &mut Writer,
    level: usize,
    key: &str,
    m: Option<&BTreeMap<String, Quantity>>,
) {
    let Some(m) = m.filter(|m| !m.is_empty()) else {
        return;
    };
    w.section(level, key);
    for (k, q) in m {
        w.kv(level + 1, k, &q.0);
    }
}

/// kubectl's label selector string: `a=b,c in (x,y),d,!e`.
pub fn selector_string(s: &LabelSelector) -> String {
    let mut parts: Vec<String> = map_lines(s.match_labels.as_ref());
    for req in s.match_expressions.as_deref().unwrap_or(&[]) {
        let values = req.values.as_deref().unwrap_or(&[]).join(",");
        parts.push(match req.operator.as_str() {
            "In" => format!("{} in ({values})", req.key),
            "NotIn" => format!("{} notin ({values})", req.key),
            "Exists" => req.key.clone(),
            "DoesNotExist" => format!("!{}", req.key),
            other => format!("{} {other} ({values})", req.key),
        });
    }
    if parts.is_empty() {
        NONE.to_string()
    } else {
        parts.join(",")
    }
}

/// Known access modes in kubectl's canonical `RWO,ROX,RWX,RWOP` order.
/// Duplicates and unknown modes are ignored; empty input produces an empty string.
pub fn access_modes_short(modes: Option<&Vec<String>>) -> String {
    let modes = modes.map(Vec::as_slice).unwrap_or_default();
    [
        ("ReadWriteOnce", "RWO"),
        ("ReadOnlyMany", "ROX"),
        ("ReadWriteMany", "RWX"),
        ("ReadWriteOncePod", "RWOP"),
    ]
    .into_iter()
    .filter_map(|(mode, short)| {
        modes
            .iter()
            .any(|candidate| candidate == mode)
            .then_some(short)
    })
    .collect::<Vec<_>>()
    .join(",")
}

const ACRONYMS: &[&str] = &[
    "api", "cidr", "dns", "id", "ip", "tls", "uid", "url", "uri", "http", "https", "tcp", "udp",
    "os", "cpu",
];

/// camelCase JSON key → kubectl-style label: `creationTimestamp` → `Creation Timestamp`,
/// `podCIDR` → `Pod CIDR`, `uid` → `UID`.
pub fn title_case(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let prev_lower = i > 0 && (chars[i - 1].is_lowercase() || chars[i - 1].is_ascii_digit());
        let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
        let prev_upper = i > 0 && chars[i - 1].is_uppercase();
        let trailing_plural_s = chars.get(i + 1) == Some(&'s') && i + 2 == chars.len();
        if c.is_uppercase()
            && !cur.is_empty()
            && (prev_lower || (prev_upper && next_lower && !trailing_plural_s))
        {
            words.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
        .iter()
        .map(|w| {
            let lower = w.to_lowercase();
            let plural_acronym = lower
                .strip_suffix('s')
                .filter(|singular| ACRONYMS.contains(singular));
            if let Some(singular) = plural_acronym {
                format!("{}s", singular.to_uppercase())
            } else if ACRONYMS.contains(&lower.as_str()) {
                w.to_uppercase()
            } else {
                let mut cs = w.chars();
                match cs.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Helpers shared by the describer unit tests. The golden harness in
/// `tests/describe.rs` carries its own copy of `normalize` because integration
/// tests cannot see `#[cfg(test)]` items.
#[cfg(test)]
pub mod test_support {
    /// Collapses alignment padding — trailing whitespace trimmed, leading
    /// whitespace kept, internal runs of 2+ spaces reduced to two — so
    /// assertions do not have to encode column widths.
    pub fn normalize(value: &str) -> String {
        let mut output = String::new();
        for line in value.lines() {
            let line = line.trim_end();
            let leading = line.len() - line.trim_start().len();
            output.push_str(&line[..leading]);
            let mut spaces = 0;
            for character in line[leading..].chars() {
                if character == ' ' {
                    spaces += 1;
                    continue;
                }
                if spaces > 0 {
                    output.push_str(if spaces > 1 { "  " } else { " " });
                    spaces = 0;
                }
                output.push(character);
            }
            output.push('\n');
        }
        output
    }

    /// The slice of `output` from the first `start` up to the following `end`.
    pub fn block<'a>(output: &'a str, start: &str, end: &str) -> &'a str {
        let start = output.find(start).unwrap();
        let end = output[start..].find(end).unwrap() + start;
        &output[start..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelectorRequirement;

    #[test]
    fn title_cases_keys() {
        assert_eq!(title_case("creationTimestamp"), "Creation Timestamp");
        assert_eq!(title_case("podCIDR"), "Pod CIDR");
        assert_eq!(title_case("uid"), "UID");
        assert_eq!(title_case("hostIP"), "Host IP");
        assert_eq!(title_case("podIPs"), "Pod IPs");
        assert_eq!(title_case("podCIDRs"), "Pod CIDRs");
        assert_eq!(title_case("containerIDs"), "Container IDs");
        assert_eq!(title_case("replicas"), "Replicas");
        assert_eq!(title_case("Status"), "Status");
    }

    #[test]
    fn selector_string_combines_labels_and_expressions() {
        let s = LabelSelector {
            match_labels: Some(
                [("app".to_string(), "web".to_string())]
                    .into_iter()
                    .collect(),
            ),
            match_expressions: Some(vec![
                LabelSelectorRequirement {
                    key: "tier".into(),
                    operator: "In".into(),
                    values: Some(vec!["a".into(), "b".into()]),
                },
                LabelSelectorRequirement {
                    key: "canary".into(),
                    operator: "DoesNotExist".into(),
                    values: None,
                },
            ]),
        };
        assert_eq!(selector_string(&s), "app=web,tier in (a,b),!canary");
        assert_eq!(selector_string(&LabelSelector::default()), "<none>");
    }

    #[test]
    fn access_modes_are_abbreviated() {
        let modes = vec![
            "ReadWriteOncePod".to_string(),
            "ReadWriteMany".to_string(),
            "ReadWriteOnce".to_string(),
            "ReadOnlyMany".to_string(),
            "ReadWriteOnce".to_string(),
            "FutureMode".to_string(),
        ];
        assert_eq!(access_modes_short(Some(&modes)), "RWO,ROX,RWX,RWOP");
        assert_eq!(access_modes_short(Some(&vec![])), "");
        assert_eq!(access_modes_short(Some(&vec!["FutureMode".into()])), "");
        assert_eq!(access_modes_short(None), "");
    }

    #[test]
    fn rfc1123z_formats_like_kubectl() {
        let t: Time = serde_json::from_str("\"2026-07-01T00:00:00Z\"").unwrap();
        assert_eq!(rfc1123z(&t), "Wed, 01 Jul 2026 00:00:00 +0000");
    }

    #[test]
    fn write_list_uses_continuations_and_none() {
        let mut w = Writer::new();
        write_list(&mut w, 0, "Labels", &["a=1".to_string(), "b=2".to_string()]);
        write_list(&mut w, 0, "Annotations", &[]);
        assert_eq!(
            w.finish(),
            "Labels:       a=1\n              b=2\nAnnotations:  <none>\n"
        );
    }
}
