use chrono::DateTime;

/// k9s-style age for table columns: 45s, 2m30s, 2h2m, 2d4h, 60d. Missing/invalid → "—", future → "0s".
/// Mirrors `src/lib/age.ts`.
pub fn age(created: Option<&str>, now_ms: i64) -> String {
    let Some(created) = created else {
        return "—".into();
    };
    let Ok(t) = DateTime::parse_from_rfc3339(created) else {
        return "—".into();
    };
    age_secs((now_ms - t.timestamp_millis()).div_euclid(1000))
}

/// Same k9s-style table formatting for an already computed number of seconds (negative → 0s).
pub fn age_secs(secs: i64) -> String {
    let mut s = secs.max(0);
    if s < 60 {
        return format!("{s}s");
    }
    let m = s / 60;
    s -= m * 60;
    if m < 60 {
        return format!("{m}m{s}s");
    }
    let h = m / 60;
    let mm = m - h * 60;
    if h < 24 {
        return format!("{h}h{mm}m");
    }
    let d = h / 24;
    let hh = h - d * 24;
    if d < 30 {
        return format!("{d}d{hh}h");
    }
    format!("{d}d")
}

/// Port of `k8s.io/apimachinery/pkg/util/duration.HumanDuration`, used for `kubectl describe` output:
/// 45s, 2m, 2m30s (under 10m), 3h, 3h5m (under 8h), 9d, 1y35d; `<invalid>` below -1s.
pub fn human_duration(secs: i64) -> String {
    if secs < -1 {
        return "<invalid>".into();
    }
    if secs < 0 {
        return "0s".into();
    }
    if secs < 60 * 2 {
        return format!("{secs}s");
    }
    let minutes = secs / 60;
    if minutes < 10 {
        let s = secs % 60;
        return if s == 0 {
            format!("{minutes}m")
        } else {
            format!("{minutes}m{s}s")
        };
    }
    if minutes < 60 * 3 {
        return format!("{minutes}m");
    }
    let hours = secs / 3600;
    if hours < 8 {
        let m = minutes % 60;
        return if m == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h{m}m")
        };
    }
    if hours < 48 {
        return format!("{hours}h");
    }
    if hours < 24 * 8 {
        let h = hours % 24;
        return if h == 0 {
            format!("{}d", hours / 24)
        } else {
            format!("{}d{h}h", hours / 24)
        };
    }
    if hours < 24 * 365 * 2 {
        return format!("{}d", hours / 24);
    }
    if hours < 24 * 365 * 8 {
        let dy = (hours / 24) % 365;
        return if dy == 0 {
            format!("{}y", hours / 24 / 365)
        } else {
            format!("{}y{dy}d", hours / 24 / 365)
        };
    }
    format!("{}y", hours / 24 / 365)
}

/// `human_duration` of `now - created`; `<unknown>` when missing or unparsable (kubectl behavior).
pub fn human_age(created: Option<&str>, now_ms: i64) -> String {
    let Some(created) = created else {
        return "<unknown>".into();
    };
    let Ok(t) = DateTime::parse_from_rfc3339(created) else {
        return "<unknown>".into();
    };
    human_duration((now_ms - t.timestamp_millis()) / 1000)
}

/// Seconds encoded by a kubectl HumanDuration string ("0s", "45s", "5m30s",
/// "3h", "2d4h", "1y20d"). Anything else — including the API server's
/// "<unknown>" / "<invalid>" placeholders — is `None`. Mirrors
/// `src/lib/events.ts::parseHumanDuration`.
pub fn parse_human_duration(text: &str) -> Option<i64> {
    const UNITS: [(u8, i64); 4] = [(b's', 1), (b'm', 60), (b'h', 3600), (b'd', 86_400)];
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let b = t.as_bytes();
    let mut total = 0i64;
    let mut i = 0;
    while i < b.len() {
        let digits_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == digits_start || i == b.len() {
            return None; // no number, or unit missing
        }
        let n: i64 = t[digits_start..i].parse().ok()?;
        let unit = b[i];
        let secs = if unit == b'y' {
            31_536_000
        } else {
            UNITS.iter().find(|(u, _)| *u == unit)?.1
        };
        total += n * secs;
        i += 1;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW_MS: i64 = 1_783_080_000_000; // 2026-07-03T12:00:00Z

    fn at(iso: &str) -> String {
        age(Some(iso), NOW_MS)
    }

    fn human_at(iso: &str) -> String {
        human_age(Some(iso), NOW_MS)
    }

    #[test]
    fn formats_k9s_style() {
        assert_eq!(at("2026-07-03T11:59:15Z"), "45s");
        assert_eq!(at("2026-07-03T11:57:30Z"), "2m30s");
        assert_eq!(at("2026-07-03T09:58:00Z"), "2h2m");
        assert_eq!(at("2026-07-01T08:00:00Z"), "2d4h");
        assert_eq!(at("2026-05-04T12:00:00Z"), "60d");
    }

    #[test]
    fn handles_edge_cases() {
        assert_eq!(age(None, NOW_MS), "—");
        assert_eq!(age(Some("garbage"), NOW_MS), "—");
        assert_eq!(at("2026-07-03T12:00:30Z"), "0s");
        assert_eq!(at("2026-07-03T11:59:15+00:00"), "45s");
    }

    #[test]
    fn age_secs_formats_buckets() {
        assert_eq!(age_secs(45), "45s");
        assert_eq!(age_secs(150), "2m30s");
        assert_eq!(age_secs(-5), "0s");
        assert_eq!(age_secs(60 * 60 * 24 * 60), "60d");
        assert_eq!(age_secs(59), "59s");
        assert_eq!(age_secs(60), "1m0s");
        assert_eq!(age_secs(3599), "59m59s");
        assert_eq!(age_secs(3600), "1h0m");
        assert_eq!(age_secs(86399), "23h59m");
        assert_eq!(age_secs(86400), "1d0h");
        assert_eq!(age_secs(86400 * 30 - 1), "29d23h");
        assert_eq!(age_secs(86400 * 30), "30d");
    }

    #[test]
    fn human_duration_matches_go_reference() {
        assert_eq!(human_duration(-5), "<invalid>");
        assert_eq!(human_duration(-1), "0s");
        assert_eq!(human_duration(0), "0s");
        assert_eq!(human_duration(119), "119s");
        assert_eq!(human_duration(120), "2m");
        assert_eq!(human_duration(150), "2m30s");
        assert_eq!(human_duration(600), "10m");
        assert_eq!(human_duration(10799), "179m");
        assert_eq!(human_duration(10800), "3h");
        assert_eq!(human_duration(11100), "3h5m");
        assert_eq!(human_duration(8 * 3600), "8h");
        assert_eq!(human_duration(47 * 3600), "47h");
        assert_eq!(human_duration(48 * 3600), "2d");
        assert_eq!(human_duration(60 * 3600), "2d12h");
        assert_eq!(human_duration(8 * 86400), "8d");
        assert_eq!(human_duration(400 * 86400), "400d");
        assert_eq!(human_duration(730 * 86400), "2y");
        assert_eq!(human_duration(765 * 86400), "2y35d");
        assert_eq!(human_duration(8 * 365 * 86400), "8y");
    }

    #[test]
    fn human_age_edge_cases() {
        assert_eq!(human_age(None, NOW_MS), "<unknown>");
        assert_eq!(human_age(Some("garbage"), NOW_MS), "<unknown>");
        assert_eq!(human_at("2026-07-03T11:55:00Z"), "5m");
        assert_eq!(human_at("2026-07-03T12:00:01.500Z"), "0s");
        assert_eq!(human_at("2026-07-03T12:00:30Z"), "<invalid>");
    }

    #[test]
    fn parse_human_duration_single_units() {
        assert_eq!(parse_human_duration("0s"), Some(0));
        assert_eq!(parse_human_duration("45s"), Some(45));
        assert_eq!(parse_human_duration("7m"), Some(420));
        assert_eq!(parse_human_duration("3h"), Some(10_800));
        assert_eq!(parse_human_duration("2d"), Some(172_800));
        assert_eq!(parse_human_duration("1y"), Some(31_536_000));
    }

    #[test]
    fn parse_human_duration_two_units() {
        assert_eq!(parse_human_duration("5m30s"), Some(330));
        assert_eq!(parse_human_duration("2h15m"), Some(8_100));
        assert_eq!(parse_human_duration("2d4h"), Some(187_200));
    }

    #[test]
    fn parse_human_duration_tolerates_whitespace() {
        assert_eq!(parse_human_duration("  12m  "), Some(720));
    }

    #[test]
    fn parse_human_duration_rejects_placeholders_and_junk() {
        assert_eq!(parse_human_duration("<unknown>"), None);
        assert_eq!(parse_human_duration("<invalid>"), None);
        assert_eq!(parse_human_duration(""), None);
        assert_eq!(parse_human_duration("soon"), None);
        assert_eq!(parse_human_duration("5x"), None);
        assert_eq!(parse_human_duration("5m ago"), None);
        assert_eq!(parse_human_duration("about 5m"), None);
    }
}
