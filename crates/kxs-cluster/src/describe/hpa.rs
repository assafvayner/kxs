use super::header::write_labels_annotations;
use super::util::{or_none, rfc1123z, UNKNOWN, UNSET};
use super::writer::Writer;
use k8s_openapi::api::autoscaling::v2::{
    HPAScalingRules, HorizontalPodAutoscaler, MetricSpec, MetricStatus, MetricValueStatus,
};

fn quantity(value: Option<&k8s_openapi::apimachinery::pkg::api::resource::Quantity>) -> &str {
    value.map(|value| value.0.as_str()).unwrap_or(UNSET)
}

fn current_average(current: Option<&MetricValueStatus>) -> &str {
    current
        .and_then(|current| current.average_value.as_ref())
        .map(|value| value.0.as_str())
        .unwrap_or(UNKNOWN)
}

fn current_value(current: Option<&MetricValueStatus>) -> &str {
    current
        .and_then(|current| current.value.as_ref())
        .map(|value| value.0.as_str())
        .unwrap_or(UNKNOWN)
}

fn current_utilization(current: Option<&MetricValueStatus>) -> String {
    match current {
        Some(MetricValueStatus {
            average_utilization: Some(utilization),
            average_value: Some(value),
            ..
        }) => format!("{utilization}% ({})", value.0),
        _ => UNKNOWN.into(),
    }
}

fn write_metric(w: &mut Writer, spec: &MetricSpec, status: Option<&MetricStatus>) {
    match spec.type_.as_str() {
        "Resource" => {
            let Some(resource) = &spec.resource else {
                return;
            };
            let current = status
                .and_then(|status| status.resource.as_ref())
                .map(|resource| &resource.current);
            let key = format!("resource {} on pods", resource.name);
            if let Some(target) = resource.target.average_value.as_ref() {
                w.kv(
                    1,
                    &key,
                    format!("{} / {}", current_average(current), target.0),
                );
            } else {
                let target = resource
                    .target
                    .average_utilization
                    .map(|utilization| format!("{utilization}%"))
                    .unwrap_or_else(|| "<auto>".into());
                w.kv(
                    1,
                    &format!("{key}  (as a percentage of request)"),
                    format!("{} / {target}", current_utilization(current)),
                );
            }
        }
        "Pods" => {
            let Some(pods) = &spec.pods else {
                return;
            };
            let current = status
                .and_then(|status| status.pods.as_ref())
                .map(|pods| &pods.current);
            w.kv(
                1,
                &format!("\"{}\" on pods", pods.metric.name),
                format!(
                    "{} / {}",
                    current_average(current),
                    quantity(pods.target.average_value.as_ref())
                ),
            );
        }
        "External" => {
            let Some(external) = &spec.external else {
                return;
            };
            let current = status
                .and_then(|status| status.external.as_ref())
                .map(|external| &external.current);
            let (kind, current, target) = if external.target.average_value.is_some() {
                (
                    "target average value",
                    current_average(current),
                    quantity(external.target.average_value.as_ref()),
                )
            } else {
                (
                    "target value",
                    current_value(current),
                    quantity(external.target.value.as_ref()),
                )
            };
            w.kv(
                1,
                &format!("\"{}\" ({kind})", external.metric.name),
                format!("{current} / {target}"),
            );
        }
        "Object" => {
            let Some(object) = &spec.object else {
                return;
            };
            let current = status
                .and_then(|status| status.object.as_ref())
                .map(|object| &object.current);
            let (target_kind, current, target) = if object.target.type_ == "AverageValue" {
                (
                    "target average value",
                    current_average(current),
                    quantity(object.target.average_value.as_ref()),
                )
            } else {
                (
                    "target value",
                    current_value(current),
                    quantity(object.target.value.as_ref()),
                )
            };
            w.kv(
                1,
                &format!(
                    "\"{}\" on {}/{} ({target_kind})",
                    object.metric.name, object.described_object.kind, object.described_object.name
                ),
                format!("{current} / {target}"),
            );
        }
        "ContainerResource" => {
            let Some(resource) = &spec.container_resource else {
                return;
            };
            let current = status
                .and_then(|status| status.container_resource.as_ref())
                .map(|resource| &resource.current);
            let key = format!(
                "resource {} of container \"{}\" on pods",
                resource.name, resource.container
            );
            if let Some(target) = &resource.target.average_value {
                w.kv(
                    1,
                    &key,
                    format!("{} / {}", current_average(current), target.0),
                );
            } else {
                let target = resource
                    .target
                    .average_utilization
                    .map(|utilization| format!("{utilization}%"))
                    .unwrap_or_else(|| "<auto>".into());
                w.kv(
                    1,
                    &format!("{key}  (as a percentage of request)"),
                    format!("{} / {target}", current_utilization(current)),
                );
            }
        }
        other => w.text(1, &format!("<unknown metric type \"{other}\">")),
    }
}

fn write_rules(w: &mut Writer, key: &str, rules: Option<&HPAScalingRules>) {
    let Some(rules) = rules else {
        return;
    };
    w.section(1, key);
    if let Some(seconds) = rules.stabilization_window_seconds {
        w.text(2, &format!("Stabilization Window: {seconds} seconds"));
    }
    if let Some(policies) = rules
        .policies
        .as_ref()
        .filter(|policies| !policies.is_empty())
    {
        w.text(
            2,
            &format!(
                "Select Policy: {}",
                rules.select_policy.as_deref().unwrap_or("Max")
            ),
        );
        w.section(2, "Policies");
        for policy in policies {
            let type_ = format!("- Type: {}", policy.type_);
            let value = format!("Value: {}", policy.value);
            let period = format!("Period: {} seconds", policy.period_seconds);
            w.cells(3, &[&type_, &value, &period]);
        }
    }
}

pub fn write(w: &mut Writer, hpa: &HorizontalPodAutoscaler) {
    let metadata = &hpa.metadata;
    let spec = hpa.spec.as_ref();
    let status = hpa.status.as_ref();

    w.kv(0, "Name", or_none(metadata.name.as_deref()));
    w.kv(0, "Namespace", or_none(metadata.namespace.as_deref()));
    write_labels_annotations(w, metadata);
    if let Some(timestamp) = &metadata.creation_timestamp {
        w.kv(0, "CreationTimestamp", rfc1123z(timestamp));
    }

    let target_kind = spec
        .map(|spec| spec.scale_target_ref.kind.as_str())
        .unwrap_or("");
    let target_name = spec
        .map(|spec| spec.scale_target_ref.name.as_str())
        .unwrap_or("");
    w.kv(0, "Reference", format!("{target_kind}/{target_name}"));
    w.kv(0, "Metrics", "( current / target )");
    let metrics = spec
        .and_then(|spec| spec.metrics.as_deref())
        .unwrap_or_default();
    let current_metrics = status
        .and_then(|status| status.current_metrics.as_deref())
        .unwrap_or_default();
    for (index, metric) in metrics.iter().enumerate() {
        write_metric(w, metric, current_metrics.get(index));
    }

    let min_replicas = spec
        .and_then(|spec| spec.min_replicas)
        .map(|replicas| replicas.to_string())
        .unwrap_or_else(|| UNSET.into());
    w.kv(0, "Min replicas", min_replicas);
    w.kv(
        0,
        "Max replicas",
        spec.map(|spec| spec.max_replicas).unwrap_or(0),
    );
    if let Some(behavior) = spec.and_then(|spec| spec.behavior.as_ref()) {
        w.section(0, "Behavior");
        write_rules(w, "Scale Up", behavior.scale_up.as_ref());
        write_rules(w, "Scale Down", behavior.scale_down.as_ref());
    }
    w.kv(
        0,
        &format!("{target_kind} pods"),
        format!(
            "{} current / {} desired",
            status
                .and_then(|status| status.current_replicas)
                .unwrap_or(0),
            status.map(|status| status.desired_replicas).unwrap_or(0)
        ),
    );

    if let Some(conditions) = status
        .and_then(|status| status.conditions.as_ref())
        .filter(|conditions| !conditions.is_empty())
    {
        w.section(0, "Conditions");
        w.cells(1, &["Type", "Status", "Reason", "Message"]);
        w.cells(1, &["----", "------", "------", "-------"]);
        for condition in conditions {
            w.cells(
                1,
                &[
                    &condition.type_,
                    &condition.status,
                    condition.reason.as_deref().unwrap_or(""),
                    condition.message.as_deref().unwrap_or(""),
                ],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::describe::util::test_support::{block, normalize};
    use serde_json::json;

    fn output(value: serde_json::Value) -> String {
        let hpa = serde_json::from_value(value).unwrap();
        let mut writer = Writer::new();
        write(&mut writer, &hpa);
        writer.finish()
    }

    #[test]
    fn missing_metrics_and_status_use_kubectl_defaults() {
        let output = output(json!({
            "metadata": {"name": "empty", "namespace": "default"},
            "spec": {
                "scaleTargetRef": {"apiVersion": "apps/v1", "kind": "Deployment", "name": "empty"},
                "maxReplicas": 4
            }
        }));

        assert_eq!(
            normalize(&output),
            concat!(
                "Name:  empty\n",
                "Namespace:  default\n",
                "Labels:  <none>\n",
                "Annotations:  <none>\n",
                "Reference:  Deployment/empty\n",
                "Metrics:  ( current / target )\n",
                "Min replicas:  <unset>\n",
                "Max replicas:  4\n",
                "Deployment pods:  0 current / 0 desired\n",
            )
        );
    }

    #[test]
    fn unknown_and_unavailable_metrics_are_visible() {
        let output = output(json!({
            "metadata": {"name": "metrics", "namespace": "default"},
            "spec": {
                "scaleTargetRef": {"apiVersion": "apps/v1", "kind": "Deployment", "name": "metrics"},
                "maxReplicas": 4,
                "metrics": [
                    {"type": "Pods", "pods": {"metric": {"name": "requests"}, "target": {"type": "AverageValue", "averageValue": "10"}}},
                    {"type": "Mystery"}
                ]
            }
        }));

        let output = normalize(&output);
        assert_eq!(
            block(&output, "Metrics:", "Min replicas:"),
            concat!(
                "Metrics:  ( current / target )\n",
                "  \"requests\" on pods:  <unknown> / 10\n",
                "  <unknown metric type \"Mystery\">\n",
            )
        );
    }

    #[test]
    fn all_metric_types_and_external_target_modes_match_kubectl() {
        let output = output(json!({
            "metadata": {"name": "metrics", "namespace": "default"},
            "spec": {
                "scaleTargetRef": {"apiVersion": "apps/v1", "kind": "Deployment", "name": "metrics"},
                "maxReplicas": 4,
                "metrics": [
                    {
                        "type": "External",
                        "external": {
                            "metric": {"name": "queue_depth", "selector": {"matchLabels": {"queue": "jobs"}}},
                            "target": {"type": "AverageValue", "averageValue": "10"}
                        }
                    },
                    {
                        "type": "External",
                        "external": {
                            "metric": {"name": "requests_total"},
                            "target": {"type": "Value", "value": "12"}
                        }
                    },
                    {
                        "type": "Pods",
                        "pods": {
                            "metric": {"name": "transactions", "selector": {"matchLabels": {"tier": "api"}}},
                            "target": {"type": "AverageValue", "averageValue": "4"}
                        }
                    },
                    {
                        "type": "Object",
                        "object": {
                            "describedObject": {"apiVersion": "networking.k8s.io/v1", "kind": "Ingress", "name": "gateway"},
                            "metric": {"name": "requests_per_second", "selector": {"matchLabels": {"route": "web"}}},
                            "target": {"type": "Value", "value": "100"}
                        }
                    },
                    {
                        "type": "Resource",
                        "resource": {
                            "name": "memory",
                            "target": {"type": "AverageValue", "averageValue": "256Mi"}
                        }
                    },
                    {
                        "type": "ContainerResource",
                        "containerResource": {
                            "name": "cpu",
                            "container": "app",
                            "target": {"type": "Utilization", "averageUtilization": 75}
                        }
                    }
                ]
            },
            "status": {
                "currentReplicas": 2,
                "desiredReplicas": 3,
                "currentMetrics": [
                    {
                        "type": "External",
                        "external": {
                            "metric": {"name": "queue_depth", "selector": {"matchLabels": {"queue": "jobs"}}},
                            "current": {"averageValue": "5"}
                        }
                    },
                    {
                        "type": "External",
                        "external": {
                            "metric": {"name": "requests_total"},
                            "current": {"value": "7"}
                        }
                    },
                    {
                        "type": "Pods",
                        "pods": {
                            "metric": {"name": "transactions", "selector": {"matchLabels": {"tier": "api"}}},
                            "current": {"averageValue": "3"}
                        }
                    },
                    {
                        "type": "Object",
                        "object": {
                            "describedObject": {"apiVersion": "networking.k8s.io/v1", "kind": "Ingress", "name": "gateway"},
                            "metric": {"name": "requests_per_second", "selector": {"matchLabels": {"route": "web"}}},
                            "current": {"value": "64"}
                        }
                    },
                    {
                        "type": "Resource",
                        "resource": {
                            "name": "memory",
                            "current": {"averageValue": "128Mi"}
                        }
                    },
                    {
                        "type": "ContainerResource",
                        "containerResource": {
                            "name": "cpu",
                            "container": "app",
                            "current": {"averageUtilization": 42, "averageValue": "210m"}
                        }
                    }
                ]
            }
        }));

        let output = normalize(&output);
        assert_eq!(
            block(&output, "Metrics:", "Min replicas:"),
            concat!(
                "Metrics:  ( current / target )\n",
                "  \"queue_depth\" (target average value):  5 / 10\n",
                "  \"requests_total\" (target value):  7 / 12\n",
                "  \"transactions\" on pods:  3 / 4\n",
                "  \"requests_per_second\" on Ingress/gateway (target value):  64 / 100\n",
                "  resource memory on pods:  128Mi / 256Mi\n",
                "  resource cpu of container \"app\" on pods  (as a percentage of request):  42% (210m) / 75%\n",
            )
        );
        assert!(!output.contains("route=web"));
        assert!(!output.contains("queue=jobs"));
        assert!(!output.contains("tier=api"));
    }

    #[test]
    fn object_and_container_statuses_are_sparse_and_positional() {
        let output = output(json!({
            "metadata": {"name": "metrics", "namespace": "default"},
            "spec": {
                "scaleTargetRef": {"apiVersion": "apps/v1", "kind": "Deployment", "name": "metrics"},
                "maxReplicas": 4,
                "metrics": [
                    {
                        "type": "Object",
                        "object": {
                            "describedObject": {"apiVersion": "v1", "kind": "Service", "name": "api"},
                            "metric": {"name": "connections"},
                            "target": {"type": "AverageValue", "averageValue": "20"}
                        }
                    },
                    {
                        "type": "ContainerResource",
                        "containerResource": {
                            "name": "memory",
                            "container": "sidecar",
                            "target": {"type": "AverageValue", "averageValue": "256Mi"}
                        }
                    }
                ]
            },
            "status": {
                "currentReplicas": 2,
                "desiredReplicas": 2,
                "currentMetrics": [
                    {
                        "type": "ContainerResource",
                        "containerResource": {
                            "name": "memory",
                            "container": "sidecar",
                            "current": {"averageValue": "128Mi"}
                        }
                    },
                    {
                        "type": "Object",
                        "object": {
                            "describedObject": {"apiVersion": "v1", "kind": "Service", "name": "api"},
                            "metric": {"name": "connections"},
                            "current": {"averageValue": "10"}
                        }
                    }
                ]
            }
        }));

        let output = normalize(&output);
        assert_eq!(
            block(&output, "Metrics:", "Min replicas:"),
            concat!(
                "Metrics:  ( current / target )\n",
                "  \"connections\" on Service/api (target average value):  <unknown> / 20\n",
                "  resource memory of container \"sidecar\" on pods:  <unknown> / 256Mi\n",
            )
        );
    }

    /// Exact bytes, so the Conditions column widths are pinned.
    #[test]
    fn conditions_table_is_column_aligned() {
        let output = output(json!({
            "metadata": {"name": "web", "namespace": "default"},
            "spec": {
                "scaleTargetRef": {"apiVersion": "apps/v1", "kind": "Deployment", "name": "web"},
                "maxReplicas": 4
            },
            "status": {
                "currentReplicas": 2,
                "desiredReplicas": 2,
                "conditions": [
                    {
                        "type": "AbleToScale",
                        "status": "True",
                        "reason": "ReadyForNewScale",
                        "message": "recommended size matches current size"
                    },
                    {
                        "type": "ScalingLimited",
                        "status": "False",
                        "reason": "DesiredWithinRange",
                        "message": "the desired count is within the acceptable range"
                    }
                ]
            }
        }));

        assert!(
            output.contains(concat!(
                "Conditions:\n",
                "  Type            Status  Reason              Message\n",
                "  ----            ------  ------              -------\n",
                "  AbleToScale     True    ReadyForNewScale    recommended size matches current size\n",
                "  ScalingLimited  False   DesiredWithinRange  the desired count is within the acceptable range\n",
            )),
            "conditions table:\n{output}"
        );
    }

    #[test]
    fn behavior_uses_literal_lines_policy_columns_and_default_select_policy() {
        let output = output(json!({
            "metadata": {"name": "behavior", "namespace": "default"},
            "spec": {
                "scaleTargetRef": {"apiVersion": "apps/v1", "kind": "Deployment", "name": "behavior"},
                "maxReplicas": 10,
                "behavior": {
                    "scaleUp": {
                        "stabilizationWindowSeconds": 10,
                        "selectPolicy": "Min"
                    },
                    "scaleDown": {
                        "policies": [
                            {"type": "Percent", "value": 100, "periodSeconds": 15},
                            {"type": "Pods", "value": 4, "periodSeconds": 60}
                        ]
                    }
                }
            },
            "status": {"currentReplicas": 3, "desiredReplicas": 2}
        }));

        assert_eq!(
            block(&output, "Behavior:\n", "Deployment pods:"),
            concat!(
                "Behavior:\n",
                "  Scale Up:\n",
                "    Stabilization Window: 10 seconds\n",
                "  Scale Down:\n",
                "    Select Policy: Max\n",
                "    Policies:\n",
                "      - Type: Percent  Value: 100  Period: 15 seconds\n",
                "      - Type: Pods     Value: 4    Period: 60 seconds\n",
            )
        );
    }
}
