use super::util::{or_none, NONE};
use super::writer::Writer;
use crate::resources::ResourceEvent;
use kxs_core::format::human_age;

/// kubectl's trailing `Events:` section, oldest first.
pub fn write_events(w: &mut Writer, events: &[ResourceEvent], now_ms: i64) {
    if events.is_empty() {
        w.kv(0, "Events", NONE);
        return;
    }
    let mut sorted: Vec<&ResourceEvent> = events.iter().collect();
    sorted.sort_by(|a, b| a.last_seen.cmp(&b.last_seen));
    w.section(0, "Events");
    w.cells(1, &["Type", "Reason", "Age", "From", "Message"]);
    w.cells(1, &["----", "------", "----", "----", "-------"]);
    for e in sorted {
        let age = event_age(e, now_ms);
        w.cells(
            1,
            &[
                &e.type_,
                &e.reason,
                &age,
                or_none(Some(&e.source)),
                &e.message,
            ],
        );
    }
}

/// `2m` or, for repeated events, `2m (x3 over 10m)`; `<unknown>` without a timestamp.
pub fn event_age(e: &ResourceEvent, now_ms: i64) -> String {
    let last = human_age(e.last_seen.as_deref(), now_ms);
    if e.count > 1 {
        let first = human_age(e.first_seen.as_deref().or(e.last_seen.as_deref()), now_ms);
        format!("{last} (x{} over {first})", e.count)
    } else {
        last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW_MS: i64 = 1_783_080_000_000; // 2026-07-03T12:00:00Z

    fn ev(
        type_: &str,
        reason: &str,
        count: i32,
        last: &str,
        first: Option<&str>,
        msg: &str,
    ) -> ResourceEvent {
        ResourceEvent {
            type_: type_.into(),
            reason: reason.into(),
            message: msg.into(),
            count,
            last_seen: Some(last.into()),
            first_seen: first.map(Into::into),
            source: "kubelet".into(),
        }
    }

    #[test]
    fn empty_events_print_none() {
        let mut w = Writer::new();
        write_events(&mut w, &[], NOW_MS);
        assert_eq!(w.finish(), "Events:  <none>\n");
    }

    #[test]
    fn events_are_a_table_sorted_oldest_first() {
        let events = vec![
            ev(
                "Warning",
                "BackOff",
                3,
                "2026-07-03T11:58:00Z",
                Some("2026-07-03T11:50:00Z"),
                "Back-off restarting",
            ),
            ev(
                "Normal",
                "Pulled",
                1,
                "2026-07-03T11:55:00Z",
                None,
                "Container image pulled",
            ),
        ];
        let mut w = Writer::new();
        write_events(&mut w, &events, NOW_MS);
        let expected = "\
Events:
  Type     Reason   Age               From     Message
  ----     ------   ----              ----     -------
  Normal   Pulled   5m                kubelet  Container image pulled
  Warning  BackOff  2m (x3 over 10m)  kubelet  Back-off restarting
";
        assert_eq!(w.finish(), expected);
    }

    #[test]
    fn event_age_includes_count_and_first_seen() {
        let e = ev(
            "Normal",
            "X",
            3,
            "2026-07-03T11:58:00Z",
            Some("2026-07-03T11:50:00Z"),
            "",
        );
        assert_eq!(event_age(&e, NOW_MS), "2m (x3 over 10m)");
        let e = ev("Normal", "X", 1, "2026-07-03T11:58:00Z", None, "");
        assert_eq!(event_age(&e, NOW_MS), "2m");
    }
}
