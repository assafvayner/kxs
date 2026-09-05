//! Log window presets shared by the logs view. Ported from
//! `src/lib/logOptions.ts`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinceOption {
    pub label: &'static str,
    pub seconds: i64,
}

/// `seconds: 0` means "all", i.e. no sinceSeconds at all.
pub const SINCE_OPTIONS: [SinceOption; 6] = [
    SinceOption {
        label: "5m",
        seconds: 300,
    },
    SinceOption {
        label: "15m",
        seconds: 900,
    },
    SinceOption {
        label: "1h",
        seconds: 3600,
    },
    SinceOption {
        label: "6h",
        seconds: 21600,
    },
    SinceOption {
        label: "24h",
        seconds: 86400,
    },
    SinceOption {
        label: "all",
        seconds: 0,
    },
];

/// Multi-pod views stream one request per pod, so they keep a smaller tail cap.
pub fn tail_options(multi: bool) -> Vec<i64> {
    if multi {
        vec![100, 200, 1000]
    } else {
        vec![100, 1000, 5000]
    }
}

pub fn default_tail(multi: bool) -> i64 {
    if multi {
        200
    } else {
        1000
    }
}

/// The API applies tailLines and sinceSeconds together, truncating the requested
/// window to the last N lines, so only one of them may be sent at a time.
pub fn log_window(since_seconds: i64, tail_lines: i64) -> (Option<i64>, Option<i64>) {
    if since_seconds > 0 {
        (None, Some(since_seconds))
    } else {
        (Some(tail_lines), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_sends_only_tail_lines_when_since_is_all() {
        assert_eq!(log_window(0, 1000), (Some(1000), None));
    }

    #[test]
    fn window_sends_only_since_seconds_when_a_window_is_chosen() {
        assert_eq!(log_window(300, 1000), (None, Some(300)));
    }

    #[test]
    fn multi_pod_views_cap_lower_than_single_pod() {
        assert!(default_tail(true) < default_tail(false));
    }

    #[test]
    fn tail_options_contain_the_default() {
        for multi in [true, false] {
            assert!(tail_options(multi).contains(&default_tail(multi)));
        }
    }

    #[test]
    fn since_options_end_with_all_and_no_other_zero() {
        assert_eq!(
            SINCE_OPTIONS[SINCE_OPTIONS.len() - 1],
            SinceOption {
                label: "all",
                seconds: 0
            }
        );
        assert_eq!(SINCE_OPTIONS.iter().filter(|o| o.seconds == 0).count(), 1);
    }
}
