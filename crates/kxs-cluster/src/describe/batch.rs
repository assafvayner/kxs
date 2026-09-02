use super::header::{write_controlled_by, write_labels_annotations};
use super::pod::write_pod_template;
use super::util::{bool_title, or_none, rfc1123z, selector_string, NONE, UNSET};
use super::writer::Writer;
use k8s_openapi::api::batch::v1::{CronJob, Job};
use kxs_core::format::human_duration;

const COMPLETED_INDEXES_SOFT_LIMIT: usize = 50;

fn opt_i32(v: Option<i32>) -> String {
    v.map(|n| n.to_string()).unwrap_or_else(|| UNSET.into())
}

fn cap_indexes_list_or_none(indexes: &str, soft_limit: usize) -> String {
    if indexes.is_empty() {
        return NONE.into();
    }
    let Some(offset) = indexes
        .as_bytes()
        .get(soft_limit..)
        .and_then(|tail| tail.iter().position(|byte| *byte == b','))
    else {
        return indexes.into();
    };
    let comma = soft_limit + offset;
    format!("{}...", &indexes[..=comma])
}

pub fn write_job(w: &mut Writer, job: &Job) {
    let meta = &job.metadata;
    let spec = job.spec.as_ref();
    let status = job.status.as_ref();
    w.kv(0, "Name", or_none(meta.name.as_deref()));
    w.kv(0, "Namespace", or_none(meta.namespace.as_deref()));
    w.kv(
        0,
        "Selector",
        spec.and_then(|s| s.selector.as_ref())
            .map(selector_string)
            .unwrap_or_else(|| UNSET.into()),
    );
    write_labels_annotations(w, meta);
    write_controlled_by(w, meta);
    if let Some(parallelism) = spec.and_then(|s| s.parallelism) {
        w.kv(0, "Parallelism", parallelism);
    }
    w.kv(0, "Completions", opt_i32(spec.and_then(|s| s.completions)));
    if let Some(m) = spec.and_then(|s| s.completion_mode.as_deref()) {
        w.kv(0, "Completion Mode", m);
    }
    if let Some(suspend) = spec.and_then(|s| s.suspend) {
        w.kv(0, "Suspend", suspend);
    }
    if let Some(backoff_limit) = spec.and_then(|s| s.backoff_limit) {
        w.kv(0, "Backoff Limit", backoff_limit);
    }
    if let Some(ttl) = spec.and_then(|s| s.ttl_seconds_after_finished) {
        w.kv(0, "TTL Seconds After Finished", ttl);
    }
    let start = status.and_then(|s| s.start_time.as_ref());
    let done = status.and_then(|s| s.completion_time.as_ref());
    if let Some(t) = start {
        w.kv(0, "Start Time", rfc1123z(t));
    }
    if let Some(t) = done {
        w.kv(0, "Completed At", rfc1123z(t));
    }
    if let (Some(s), Some(d)) = (start, done) {
        w.kv(0, "Duration", human_duration((d.0 - s.0).num_seconds()));
    }
    if let Some(deadline) = spec.and_then(|s| s.active_deadline_seconds) {
        w.kv(0, "Active Deadline Seconds", format!("{deadline}s"));
    }
    let n = |v: Option<i32>| v.unwrap_or(0);
    let active = n(status.and_then(|s| s.active));
    let succeeded = n(status.and_then(|s| s.succeeded));
    let failed = n(status.and_then(|s| s.failed));
    let statuses = match status.and_then(|s| s.ready) {
        Some(ready) => {
            format!("{active} Active ({ready} Ready) / {succeeded} Succeeded / {failed} Failed")
        }
        None => format!("{active} Active / {succeeded} Succeeded / {failed} Failed"),
    };
    w.kv(0, "Pods Statuses", statuses);
    if spec.and_then(|s| s.completion_mode.as_deref()) == Some("Indexed") {
        w.kv(
            0,
            "Completed Indexes",
            cap_indexes_list_or_none(
                status
                    .and_then(|s| s.completed_indexes.as_deref())
                    .unwrap_or_default(),
                COMPLETED_INDEXES_SOFT_LIMIT,
            ),
        );
    }
    if let Some(s) = spec {
        write_pod_template(w, 0, &s.template);
    }
}

pub fn write_cronjob(w: &mut Writer, cj: &CronJob) {
    let meta = &cj.metadata;
    let spec = cj.spec.as_ref();
    let status = cj.status.as_ref();
    w.kv(0, "Name", or_none(meta.name.as_deref()));
    w.kv(0, "Namespace", or_none(meta.namespace.as_deref()));
    write_labels_annotations(w, meta);
    w.kv(
        0,
        "Schedule",
        spec.map(|s| s.schedule.as_str()).unwrap_or(""),
    );
    w.kv(
        0,
        "Concurrency Policy",
        spec.and_then(|s| s.concurrency_policy.as_deref())
            .unwrap_or(UNSET),
    );
    w.kv(
        0,
        "Suspend",
        spec.and_then(|s| s.suspend)
            .map(bool_title)
            .unwrap_or(UNSET),
    );
    w.kv(
        0,
        "Time Zone",
        spec.and_then(|s| s.time_zone.as_deref()).unwrap_or(UNSET),
    );
    w.kv(
        0,
        "Successful Job History Limit",
        opt_i32(spec.and_then(|s| s.successful_jobs_history_limit)),
    );
    w.kv(
        0,
        "Failed Job History Limit",
        opt_i32(spec.and_then(|s| s.failed_jobs_history_limit)),
    );
    w.kv(
        0,
        "Starting Deadline Seconds",
        spec.and_then(|s| s.starting_deadline_seconds)
            .map(|n| format!("{n}s"))
            .unwrap_or_else(|| UNSET.into()),
    );
    let job_spec = spec.and_then(|s| s.job_template.spec.as_ref());
    w.kv(
        0,
        "Selector",
        job_spec
            .and_then(|j| j.selector.as_ref())
            .map(selector_string)
            .unwrap_or_else(|| UNSET.into()),
    );
    w.kv(
        0,
        "Parallelism",
        opt_i32(job_spec.and_then(|j| j.parallelism)),
    );
    w.kv(
        0,
        "Completions",
        opt_i32(job_spec.and_then(|j| j.completions)),
    );
    if let Some(deadline) = job_spec.and_then(|j| j.active_deadline_seconds) {
        w.kv(0, "Active Deadline Seconds", format!("{deadline}s"));
    }
    if let Some(j) = job_spec {
        write_pod_template(w, 0, &j.template);
    }
    w.kv(
        0,
        "Last Schedule Time",
        status
            .and_then(|s| s.last_schedule_time.as_ref())
            .map(rfc1123z)
            .unwrap_or_else(|| UNSET.into()),
    );
    let active: Vec<&str> = status
        .and_then(|s| s.active.as_deref())
        .unwrap_or(&[])
        .iter()
        .filter_map(|r| r.name.as_deref())
        .collect();
    w.kv(
        0,
        "Active Jobs",
        if active.is_empty() {
            NONE.to_string()
        } else {
            active.join(", ")
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn job(spec: Value, status: Value) -> Job {
        serde_json::from_value(json!({
            "metadata": {"name": "job", "namespace": "default"},
            "spec": spec,
            "status": status,
        }))
        .unwrap()
    }

    fn cronjob(spec: Value, status: Value) -> CronJob {
        serde_json::from_value(json!({
            "metadata": {"name": "cron", "namespace": "default"},
            "spec": spec,
            "status": status,
        }))
        .unwrap()
    }

    fn output_job(job: &Job) -> String {
        let mut w = Writer::new();
        write_job(&mut w, job);
        w.finish()
    }

    fn output_cronjob(cronjob: &CronJob) -> String {
        let mut w = Writer::new();
        write_cronjob(&mut w, cronjob);
        w.finish()
    }

    fn field<'a>(output: &'a str, key: &str) -> Option<&'a str> {
        let prefix = format!("{key}:");
        output
            .lines()
            .find_map(|line| line.trim_start().strip_prefix(&prefix).map(str::trim_start))
    }

    fn position(output: &str, key: &str) -> usize {
        let prefix = format!("{key}:");
        output
            .lines()
            .position(|line| line.trim_start().starts_with(&prefix))
            .unwrap_or_else(|| panic!("missing {key:?} in:\n{output}"))
    }

    fn template_spec(extra: Value) -> Value {
        let mut spec = json!({
            "template": {"spec": {"containers": [], "restartPolicy": "Never"}}
        });
        spec.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        spec
    }

    #[test]
    fn job_ready_count_is_only_printed_when_reported() {
        let spec = template_spec(json!({}));
        let without_ready = output_job(&job(
            spec.clone(),
            json!({"active": 2, "succeeded": 3, "failed": 4}),
        ));
        let with_ready = output_job(&job(
            spec,
            json!({"active": 2, "ready": 1, "succeeded": 3, "failed": 4}),
        ));

        assert_eq!(
            field(&without_ready, "Pods Statuses"),
            Some("2 Active / 3 Succeeded / 4 Failed")
        );
        assert_eq!(
            field(&with_ready, "Pods Statuses"),
            Some("2 Active (1 Ready) / 3 Succeeded / 4 Failed")
        );
    }

    #[test]
    fn job_optional_fields_are_printed_in_kubectl_order() {
        let output = output_job(&job(
            template_spec(json!({
                "completionMode": "NonIndexed",
                "suspend": true,
                "backoffLimit": 6,
                "ttlSecondsAfterFinished": 30,
                "activeDeadlineSeconds": 120
            })),
            json!({
                "startTime": "2026-07-01T00:00:00Z",
                "completionTime": "2026-07-01T00:01:00Z"
            }),
        ));

        assert_eq!(field(&output, "Parallelism"), None);
        assert_eq!(field(&output, "Suspend"), Some("true"));
        assert_eq!(field(&output, "Backoff Limit"), Some("6"));
        assert_eq!(field(&output, "TTL Seconds After Finished"), Some("30"));
        assert_eq!(field(&output, "Active Deadline Seconds"), Some("120s"));
        assert!(position(&output, "Completion Mode") < position(&output, "Suspend"));
        assert!(position(&output, "Suspend") < position(&output, "Backoff Limit"));
        assert!(
            position(&output, "Backoff Limit") < position(&output, "TTL Seconds After Finished")
        );
        assert!(position(&output, "TTL Seconds After Finished") < position(&output, "Start Time"));
        assert!(position(&output, "Start Time") < position(&output, "Completed At"));
        assert!(position(&output, "Completed At") < position(&output, "Duration"));
        assert!(position(&output, "Duration") < position(&output, "Active Deadline Seconds"));
        assert!(position(&output, "Active Deadline Seconds") < position(&output, "Pods Statuses"));
    }

    #[test]
    fn indexed_job_completed_indexes_are_empty_or_capped_at_the_next_comma() {
        let empty = output_job(&job(
            template_spec(json!({"completionMode": "Indexed"})),
            json!({}),
        ));
        let long = output_job(&job(
            template_spec(json!({"completionMode": "Indexed"})),
            json!({
                "completedIndexes": "0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22"
            }),
        ));

        assert_eq!(field(&empty, "Completed Indexes"), Some(NONE));
        assert_eq!(
            field(&long, "Completed Indexes"),
            Some("0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,...")
        );
    }

    #[test]
    fn cronjob_prints_timezone_and_template_active_deadline() {
        let output = output_cronjob(&cronjob(
            json!({
                "schedule": "0 2 * * *",
                "concurrencyPolicy": "Forbid",
                "suspend": false,
                "timeZone": "Etc/UTC",
                "jobTemplate": {"spec": template_spec(json!({"activeDeadlineSeconds": 45}))}
            }),
            json!({}),
        ));

        assert_eq!(field(&output, "Time Zone"), Some("Etc/UTC"));
        assert_eq!(field(&output, "Active Deadline Seconds"), Some("45s"));
        assert!(position(&output, "Suspend") < position(&output, "Time Zone"));
        assert!(position(&output, "Completions") < position(&output, "Active Deadline Seconds"));
        assert!(position(&output, "Active Deadline Seconds") < position(&output, "Pod Template"));
    }

    #[test]
    fn cronjob_unset_fields_and_active_jobs_match_kubectl() {
        let spec = json!({
            "schedule": "0 2 * * *",
            "jobTemplate": {"spec": template_spec(json!({}))}
        });
        let multiple = output_cronjob(&cronjob(
            spec.clone(),
            json!({"active": [{"name": "job-b"}, {"name": "job-a"}]}),
        ));
        let empty = output_cronjob(&cronjob(spec, json!({"active": []})));

        assert_eq!(field(&multiple, "Concurrency Policy"), Some(UNSET));
        assert_eq!(field(&multiple, "Suspend"), Some(UNSET));
        assert_eq!(field(&multiple, "Time Zone"), Some(UNSET));
        assert_eq!(field(&multiple, "Active Jobs"), Some("job-b, job-a"));
        assert_eq!(field(&empty, "Active Jobs"), Some(NONE));
    }
}
