//! Utilization formatting: usage vs. request/allocatable, with thresholds.
//! Ported from `src/lib/utilization.ts`; the CSS classes become a `Weight`
//! the TUI maps to theme colors.

pub const NO_VALUE: &str = "—";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utilization {
    pub text: String,
    pub cls: &'static str,
}

/// Percent of `total`, rounded. `None` when `total` is unknown or zero.
pub fn percent(used: u64, total: Option<u64>) -> Option<u64> {
    let total = total?;
    if total == 0 {
        return None;
    }
    Some(((used as f64 / total as f64) * 100.0).round() as u64)
}

/// Threshold class for a percentage: >100% bad, >80% warn, otherwise none.
pub fn util_class(pct: Option<u64>) -> &'static str {
    match pct {
        None => "",
        Some(p) if p > 100 => "st-bad",
        Some(p) if p > 80 => "st-warn",
        _ => "",
    }
}

fn format(used: Option<u64>, unit: &str, total: Option<u64>) -> Utilization {
    let Some(used) = used else {
        return Utilization {
            text: NO_VALUE.into(),
            cls: "",
        };
    };
    let pct = percent(used, total);
    let text = match pct {
        None => format!("{used}{unit}"),
        Some(p) => format!("{used}{unit} {p}%"),
    };
    Utilization {
        text,
        cls: util_class(pct),
    }
}

/// CPU cell: "123m", or "123m 49%" when the request/allocatable is known.
pub fn cpu_util(used_millis: Option<u64>, total_millis: Option<u64>) -> Utilization {
    format(used_millis, "m", total_millis)
}

/// Memory cell: "45Mi", or "45Mi 35%" when the request/allocatable is known.
pub fn mem_util(used_mib: Option<u64>, total_mib: Option<u64>) -> Utilization {
    format(used_mib, "Mi", total_mib)
}

/// "used/total unit pct%" for the node rows, e.g. "412m/4000m 10%".
pub fn of_total(used: u64, total: Option<u64>, unit: &str) -> Utilization {
    let pct = percent(used, total);
    let text = match (pct, total) {
        (Some(p), Some(t)) => format!("{used}{unit}/{t}{unit} {p}%"),
        _ => format!("{used}{unit}/{NO_VALUE}"),
    };
    Utilization {
        text,
        cls: util_class(pct),
    }
}

/// A fixed-width usage bar: `▮` filled up to the percentage, `▯` for the rest.
pub fn bar(pct: Option<u64>, width: usize) -> String {
    let filled = match pct {
        None => 0,
        Some(p) => ((p.min(100) as usize) * width) / 100,
    };
    let mut out = String::new();
    out.extend(std::iter::repeat_n('▮', filled));
    out.extend(std::iter::repeat_n('▯', width - filled));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_rounds_against_the_total() {
        assert_eq!(percent(123, Some(250)), Some(49));
        assert_eq!(percent(0, Some(250)), Some(0));
        assert_eq!(percent(300, Some(250)), Some(120));
    }

    #[test]
    fn percent_is_none_without_a_usable_total() {
        assert_eq!(percent(123, None), None);
        assert_eq!(percent(123, Some(0)), None);
    }

    #[test]
    fn util_class_applies_the_80_100_thresholds() {
        assert_eq!(util_class(Some(0)), "");
        assert_eq!(util_class(Some(80)), "");
        assert_eq!(util_class(Some(81)), "st-warn");
        assert_eq!(util_class(Some(100)), "st-warn");
        assert_eq!(util_class(Some(101)), "st-bad");
        assert_eq!(util_class(None), "");
    }

    #[test]
    fn cpu_mem_show_usage_with_percent_when_known() {
        assert_eq!(cpu_util(Some(123), Some(250)).text, "123m 49%");
        assert_eq!(cpu_util(Some(230), Some(250)).cls, "st-warn");
        assert_eq!(cpu_util(Some(300), Some(250)).cls, "st-bad");
        assert_eq!(mem_util(Some(45), Some(128)).text, "45Mi 35%");
    }

    #[test]
    fn bare_usage_without_a_request() {
        assert_eq!(cpu_util(Some(123), None).text, "123m");
        assert_eq!(mem_util(Some(45), None).text, "45Mi");
    }

    #[test]
    fn dash_when_metrics_unavailable() {
        assert_eq!(cpu_util(None, Some(250)).text, "—");
        assert_eq!(mem_util(None, Some(128)).text, "—");
    }

    #[test]
    fn of_total_renders_used_over_allocatable() {
        assert_eq!(of_total(412, Some(4000), "m").text, "412m/4000m 10%");
        assert_eq!(of_total(3800, Some(4000), "m").cls, "st-warn");
        assert_eq!(of_total(412, None, "m").text, "412m/—");
    }
}
