//! Wall-clock helpers. The workspace's chrono has no `clock` feature (no local
//! timezone); UTC now comes in via k8s-openapi's re-export.

use k8s_openapi::chrono::Utc;

/// Current UTC time as epoch milliseconds.
pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}
