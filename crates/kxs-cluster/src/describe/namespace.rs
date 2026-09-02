use super::header::write_header;
use super::util::{or_none, rfc1123z};
use super::writer::Writer;
use k8s_openapi::api::core::v1::{LimitRange, Namespace, NamespaceCondition, ResourceQuota};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use std::collections::{BTreeMap, BTreeSet};

/// Namespace output intentionally omits Events, matching kubectl.
pub fn write(
    w: &mut Writer,
    namespace: &Namespace,
    quotas: &[ResourceQuota],
    limit_ranges: &[LimitRange],
) {
    write_header(w, &namespace.metadata, false);
    w.kv(
        0,
        "Status",
        namespace
            .status
            .as_ref()
            .and_then(|status| status.phase.as_deref())
            .unwrap_or(""),
    );
    write_conditions(
        w,
        namespace
            .status
            .as_ref()
            .and_then(|status| status.conditions.as_deref())
            .unwrap_or(&[]),
    );

    w.text(0, "");
    write_quotas(w, quotas);
    w.text(0, "");
    write_limit_ranges(w, limit_ranges);
}

fn write_quotas(w: &mut Writer, quotas: &[ResourceQuota]) {
    if quotas.is_empty() {
        w.text(0, "No resource quota.");
        return;
    }

    w.text(0, "Resource Quotas");
    let mut quotas: Vec<&ResourceQuota> = quotas.iter().collect();
    quotas.sort_by(|a, b| a.metadata.name.cmp(&b.metadata.name));
    for quota in quotas {
        w.kv(1, "Name", or_none(quota.metadata.name.as_deref()));
        let mut scopes = quota
            .spec
            .as_ref()
            .and_then(|spec| spec.scopes.clone())
            .unwrap_or_default();
        scopes.sort();
        if !scopes.is_empty() {
            w.kv(1, "Scopes", scopes.join(", "));
            for scope in &scopes {
                if let Some(help) = scope_help(scope) {
                    w.text(1, &format!("* {help}"));
                }
            }
        }
        w.cells(1, &["Resource", "Used", "Hard"]);
        w.cells(1, &["--------", "---", "---"]);
        let empty = BTreeMap::new();
        let status = quota.status.as_ref();
        let hard = status
            .and_then(|status| status.hard.as_ref())
            .unwrap_or(&empty);
        let used = status
            .and_then(|status| status.used.as_ref())
            .unwrap_or(&empty);
        for (resource, hard_quantity) in hard {
            let used_quantity = used
                .get(resource)
                .map(|quantity| quantity.0.as_str())
                .unwrap_or("0");
            w.cells(1, &[resource, used_quantity, &hard_quantity.0]);
        }
    }
}

fn scope_help(scope: &str) -> Option<&'static str> {
    match scope {
        "Terminating" => Some("Matches all pods that have an active deadline. These pods have a limited lifespan on a node before being actively terminated by the system."),
        "NotTerminating" => Some("Matches all pods that do not have an active deadline. These pods usually include long running pods whose container command is not expected to terminate."),
        "BestEffort" => Some("Matches all pods that do not have resource requirements set. These pods have a best effort quality of service."),
        "NotBestEffort" => Some("Matches all pods that have at least one resource requirement set. These pods have a burstable or guaranteed quality of service."),
        _ => None,
    }
}

fn write_conditions(w: &mut Writer, conditions: &[NamespaceCondition]) {
    if conditions.is_empty() {
        return;
    }

    w.section(0, "Conditions");
    w.cells(
        1,
        &["Type", "Status", "LastTransitionTime", "Reason", "Message"],
    );
    w.cells(
        1,
        &["----", "------", "------------------", "------", "-------"],
    );
    for condition in conditions {
        let transition = condition
            .last_transition_time
            .as_ref()
            .map(rfc1123z)
            .unwrap_or_else(|| "<unknown>".to_string());
        w.cells(
            1,
            &[
                &condition.type_,
                &condition.status,
                &transition,
                condition.reason.as_deref().unwrap_or(""),
                condition.message.as_deref().unwrap_or(""),
            ],
        );
    }
}

fn write_limit_ranges(w: &mut Writer, limit_ranges: &[LimitRange]) {
    if limit_ranges.is_empty() {
        w.text(0, "No LimitRange resource.");
        return;
    }

    w.text(0, "Resource Limits");
    w.cells(
        1,
        &[
            "Type",
            "Resource",
            "Min",
            "Max",
            "Default Request",
            "Default Limit",
            "Max Limit/Request Ratio",
        ],
    );
    w.cells(
        1,
        &[
            "----",
            "--------",
            "---",
            "---",
            "---------------",
            "-------------",
            "-----------------------",
        ],
    );

    for limit_range in limit_ranges {
        let Some(spec) = limit_range.spec.as_ref() else {
            continue;
        };
        for item in &spec.limits {
            let mut resources = BTreeSet::new();
            for values in [
                &item.min,
                &item.max,
                &item.default_request,
                &item.default,
                &item.max_limit_request_ratio,
            ] {
                resources.extend(values.iter().flat_map(|values| values.keys()));
            }

            for resource in resources {
                let min = quantity_or_dash(&item.min, resource);
                let max = quantity_or_dash(&item.max, resource);
                let default_request = quantity_or_dash(&item.default_request, resource);
                let default_limit = quantity_or_dash(&item.default, resource);
                let ratio = quantity_or_dash(&item.max_limit_request_ratio, resource);
                w.cells(
                    1,
                    &[
                        &item.type_,
                        resource,
                        min,
                        max,
                        default_request,
                        default_limit,
                        ratio,
                    ],
                );
            }
        }
    }
}

fn quantity_or_dash<'a>(values: &'a Option<BTreeMap<String, Quantity>>, key: &str) -> &'a str {
    values
        .as_ref()
        .and_then(|values| values.get(key))
        .map(|quantity| quantity.0.as_str())
        .unwrap_or("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn from_value<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
        serde_json::from_value(value).unwrap()
    }

    fn normalize(value: &str) -> String {
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

    #[test]
    fn namespace_prints_conditions_sorted_quotas_scopes_and_limits() {
        let namespace: Namespace = from_value(json!({
            "metadata": {"name": "work"},
            "status": {
                "phase": "Terminating",
                "conditions": [{
                    "type": "NamespaceDeletionDiscoveryFailure",
                    "status": "False",
                    "lastTransitionTime": "2026-07-01T00:00:00Z",
                    "reason": "ResourcesDiscovered",
                    "message": "All resources discovered"
                }]
            }
        }));
        let quotas = vec![
            from_value(json!({
                "metadata": {"name": "z-quota"},
                "spec": {"scopes": ["PriorityClass", "NotTerminating", "BestEffort"]},
                "status": {"hard": {"pods": "10"}, "used": {"pods": "2"}}
            })),
            from_value(json!({
                "metadata": {"name": "a-quota"},
                "status": {"hard": {"requests.cpu": "2"}, "used": {}}
            })),
        ];
        let limit_ranges = vec![from_value(json!({
            "metadata": {"name": "limits"},
            "spec": {"limits": [{
                "type": "Container",
                "min": {"cpu": "50m"},
                "max": {"memory": "1Gi"},
                "defaultRequest": {"cpu": "100m"},
                "default": {"memory": "512Mi"}
            }]}
        }))];

        let mut writer = Writer::new();
        write(&mut writer, &namespace, &quotas, &limit_ranges);
        let output = normalize(&writer.finish());

        assert_eq!(
            output,
            concat!(
                "Name:  work\n",
                "Labels:  <none>\n",
                "Annotations:  <none>\n",
                "Status:  Terminating\n",
                "Conditions:\n",
                "  Type  Status  LastTransitionTime  Reason  Message\n",
                "  ----  ------  ------------------  ------  -------\n",
                "  NamespaceDeletionDiscoveryFailure  False  Wed, 01 Jul 2026 00:00:00 +0000  ResourcesDiscovered  All resources discovered\n",
                "\n",
                "Resource Quotas\n",
                "  Name:  a-quota\n",
                "  Resource  Used  Hard\n",
                "  --------  ---  ---\n",
                "  requests.cpu  0  2\n",
                "  Name:  z-quota\n",
                "  Scopes:  BestEffort, NotTerminating, PriorityClass\n",
                "  * Matches all pods that do not have resource requirements set. These pods have a best effort quality of service.\n",
                "  * Matches all pods that do not have an active deadline. These pods usually include long running pods whose container command is not expected to terminate.\n",
                "  Resource  Used  Hard\n",
                "  --------  ---  ---\n",
                "  pods  2  10\n",
                "\n",
                "Resource Limits\n",
                "  Type  Resource  Min  Max  Default Request  Default Limit  Max Limit/Request Ratio\n",
                "  ----  --------  ---  ---  ---------------  -------------  -----------------------\n",
                "  Container  cpu  50m  -  100m  -  -\n",
                "  Container  memory  -  1Gi  -  512Mi  -\n",
            )
        );
    }

    #[test]
    fn sparse_namespace_leaves_phase_blank() {
        let mut writer = Writer::new();
        write(&mut writer, &Namespace::default(), &[], &[]);
        assert_eq!(
            normalize(&writer.finish()),
            "Name:  <none>\nLabels:  <none>\nAnnotations:  <none>\nStatus:\n\nNo resource quota.\n\nNo LimitRange resource.\n"
        );
    }
}
