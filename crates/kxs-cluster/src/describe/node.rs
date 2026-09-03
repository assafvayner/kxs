use super::header::write_labels_annotations;
use super::util::{or_none, rfc1123z, write_list, write_quantities, NONE, UNSET};
use super::writer::Writer;
use k8s_openapi::api::coordination::v1::Lease;
use k8s_openapi::api::core::v1::{Container, ContainerStatus, Node, Pod};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{MicroTime, Time};
use kxs_core::format::human_age;
use std::collections::{BTreeMap, BTreeSet};

const ROLE_PREFIX: &str = "node-role.kubernetes.io/";
const LEGACY_ROLE_LABEL: &str = "kubernetes.io/role";
const NANO: i128 = 1_000_000_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum QuantityFormat {
    #[default]
    Decimal,
    DecimalExponent,
    Binary,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Amount {
    nanos: i128,
    format: QuantityFormat,
}

type ResourceAmounts = BTreeMap<String, Amount>;

impl Amount {
    fn parse(quantity: &Quantity) -> Option<Self> {
        let input = quantity.0.trim();
        let bytes = input.as_bytes();
        if bytes.is_empty() {
            return None;
        }
        let mut index = usize::from(matches!(bytes[0], b'+' | b'-'));
        let negative = bytes[0] == b'-';
        let number_start = index;
        let mut decimal = None;
        let mut digits = 0usize;
        while index < bytes.len() {
            match bytes[index] {
                b'0'..=b'9' => digits += 1,
                b'.' if decimal.is_none() => decimal = Some(index),
                _ => break,
            }
            index += 1;
        }
        if digits == 0 {
            return None;
        }
        let number_end = index;
        let mut exponent = 0i32;
        let mut has_exponent = false;
        if index < bytes.len() && matches!(bytes[index], b'e' | b'E') {
            let exponent_marker = index;
            index += 1;
            let exponent_start = index;
            if index < bytes.len() && matches!(bytes[index], b'+' | b'-') {
                index += 1;
            }
            let exponent_digits = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            if index > exponent_digits {
                exponent = input[exponent_start..index].parse().ok()?;
                has_exponent = true;
            } else {
                index = exponent_marker;
            }
        }
        let suffix = &input[index..];
        let (suffix_exponent, binary_power, mut format) = match suffix {
            "" => (0, 0, QuantityFormat::Decimal),
            "n" => (-9, 0, QuantityFormat::Decimal),
            "u" => (-6, 0, QuantityFormat::Decimal),
            "m" => (-3, 0, QuantityFormat::Decimal),
            "k" => (3, 0, QuantityFormat::Decimal),
            "M" => (6, 0, QuantityFormat::Decimal),
            "G" => (9, 0, QuantityFormat::Decimal),
            "T" => (12, 0, QuantityFormat::Decimal),
            "P" => (15, 0, QuantityFormat::Decimal),
            "E" => (18, 0, QuantityFormat::Decimal),
            "Ki" => (0, 1, QuantityFormat::Binary),
            "Mi" => (0, 2, QuantityFormat::Binary),
            "Gi" => (0, 3, QuantityFormat::Binary),
            "Ti" => (0, 4, QuantityFormat::Binary),
            "Pi" => (0, 5, QuantityFormat::Binary),
            "Ei" => (0, 6, QuantityFormat::Binary),
            _ => return None,
        };
        if has_exponent && !suffix.is_empty() {
            return None;
        }
        if has_exponent {
            format = QuantityFormat::DecimalExponent;
        }
        let fraction_digits = i32::try_from(decimal.map_or(0, |dot| number_end - dot - 1)).ok()?;
        let mut coefficient = 0i128;
        for byte in bytes[number_start..number_end]
            .iter()
            .copied()
            .filter(u8::is_ascii_digit)
        {
            coefficient = coefficient
                .checked_mul(10)?
                .checked_add(i128::from(byte - b'0'))?;
        }
        let binary_multiplier = 1024i128.checked_pow(binary_power)?;
        let mut magnitude = coefficient.checked_mul(binary_multiplier)?;
        let decimal_power = 9i32
            .checked_add(suffix_exponent)?
            .checked_add(exponent)?
            .checked_sub(fraction_digits)?;
        if decimal_power >= 0 {
            magnitude = magnitude.checked_mul(10i128.checked_pow(decimal_power as u32)?)?;
        } else {
            let divisor = 10i128.checked_pow(decimal_power.unsigned_abs())?;
            magnitude = magnitude / divisor + i128::from(magnitude % divisor != 0);
        }
        Some(Self {
            nanos: if negative {
                magnitude.checked_neg()?
            } else {
                magnitude
            },
            format,
        })
    }

    fn canonical(self) -> String {
        if self.nanos == 0 {
            return "0".to_string();
        }
        if self.format == QuantityFormat::Binary && self.nanos % NANO == 0 {
            let bytes = self.nanos / NANO;
            for (power, suffix) in [
                (6, "Ei"),
                (5, "Pi"),
                (4, "Ti"),
                (3, "Gi"),
                (2, "Mi"),
                (1, "Ki"),
            ] {
                let unit = 1024i128.pow(power);
                if bytes % unit == 0 {
                    return format!("{}{suffix}", bytes / unit);
                }
            }
            return bytes.to_string();
        }
        if self.format == QuantityFormat::DecimalExponent {
            let mut mantissa = self.nanos;
            let mut exponent = -9;
            while mantissa % 10 == 0 {
                mantissa /= 10;
                exponent += 1;
            }
            while exponent % 3 != 0 {
                mantissa *= 10;
                exponent -= 1;
            }
            return if exponent == 0 {
                mantissa.to_string()
            } else {
                format!("{mantissa}e{exponent}")
            };
        }
        for (exponent, suffix) in [
            (18, "E"),
            (15, "P"),
            (12, "T"),
            (9, "G"),
            (6, "M"),
            (3, "k"),
            (0, ""),
            (-3, "m"),
            (-6, "u"),
            (-9, "n"),
        ] {
            let unit = 10i128.pow((9 + exponent) as u32);
            if self.nanos % unit == 0 {
                return format!("{}{suffix}", self.nanos / unit);
            }
        }
        unreachable!("quantities are rounded to nanounits")
    }

    fn rounded_units(self, nanos_per_unit: i128) -> i128 {
        let quotient = self.nanos / nanos_per_unit;
        let remainder = self.nanos % nanos_per_unit;
        quotient
            + if remainder > 0 {
                1
            } else if remainder < 0 {
                -1
            } else {
                0
            }
    }
}

fn amounts(values: Option<&BTreeMap<String, Quantity>>) -> ResourceAmounts {
    values
        .into_iter()
        .flatten()
        .filter_map(|(name, quantity)| Amount::parse(quantity).map(|amount| (name.clone(), amount)))
        .collect()
}

fn add_amounts(target: &mut ResourceAmounts, values: &ResourceAmounts) {
    for (name, amount) in values {
        target
            .entry(name.clone())
            .and_modify(|current| {
                if current.nanos == 0 {
                    current.format = amount.format;
                }
                current.nanos = current.nanos.saturating_add(amount.nanos);
            })
            .or_insert(*amount);
    }
}

fn max_amounts(target: &mut ResourceAmounts, values: &ResourceAmounts) {
    for (name, amount) in values {
        target
            .entry(name.clone())
            .and_modify(|current| {
                if amount.nanos > current.nanos {
                    *current = *amount;
                }
            })
            .or_insert(*amount);
    }
}

fn max_lists(lists: &[ResourceAmounts]) -> ResourceAmounts {
    let mut result = ResourceAmounts::new();
    for list in lists {
        max_amounts(&mut result, list);
    }
    result
}

fn status_for<'a>(pod: &'a Pod, name: &str) -> Option<&'a ContainerStatus> {
    let status = pod.status.as_ref()?;
    status
        .container_statuses
        .iter()
        .flatten()
        .chain(status.init_container_statuses.iter().flatten())
        .find(|status| status.name == name)
}

fn resize_infeasible(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|status| status.conditions.as_ref())
        .into_iter()
        .flatten()
        .any(|condition| {
            condition.type_ == "PodResizePending"
                && condition.reason.as_deref() == Some("Infeasible")
        })
}

fn container_amounts(
    container: &Container,
    status: Option<&ContainerStatus>,
    requests: bool,
    use_status: bool,
    infeasible: bool,
) -> ResourceAmounts {
    let spec = container.resources.as_ref();
    let spec_values = amounts(if requests {
        spec.and_then(|resources| resources.requests.as_ref())
    } else {
        spec.and_then(|resources| resources.limits.as_ref())
    });
    if !requests || !use_status {
        return spec_values;
    }
    let actuated = amounts(status.and_then(|status| status.resources.as_ref()?.requests.as_ref()));
    let allocated = amounts(status.and_then(|status| status.allocated_resources.as_ref()));
    if infeasible {
        max_lists(&[actuated, allocated])
    } else {
        max_lists(&[spec_values, actuated, allocated])
    }
}

fn aggregate_containers(pod: &Pod, requests: bool, use_status: bool) -> ResourceAmounts {
    let Some(spec) = pod.spec.as_ref() else {
        return ResourceAmounts::new();
    };
    let infeasible = resize_infeasible(pod);
    let mut result = ResourceAmounts::new();
    for container in &spec.containers {
        add_amounts(
            &mut result,
            &container_amounts(
                container,
                status_for(pod, &container.name),
                requests,
                use_status,
                infeasible,
            ),
        );
    }
    let mut restartable = ResourceAmounts::new();
    let mut init_peak = ResourceAmounts::new();
    for container in spec.init_containers.iter().flatten() {
        let restartable_init = container.restart_policy.as_deref() == Some("Always");
        let mut current = container_amounts(
            container,
            status_for(pod, &container.name),
            requests,
            use_status && restartable_init,
            infeasible,
        );
        if restartable_init {
            add_amounts(&mut result, &current);
            add_amounts(&mut restartable, &current);
            current = restartable.clone();
        } else {
            add_amounts(&mut current, &restartable);
        }
        max_amounts(&mut init_peak, &current);
    }
    max_amounts(&mut result, &init_peak);
    result
}

fn supported_pod_level(name: &str) -> bool {
    matches!(name, "cpu" | "memory") || name.starts_with("hugepages-")
}

fn pod_resource_amounts(pod: &Pod, use_status: bool) -> (ResourceAmounts, ResourceAmounts) {
    let mut requests = aggregate_containers(pod, true, use_status);
    let mut limits = aggregate_containers(pod, false, use_status);
    let spec = pod.spec.as_ref();
    if let Some(resources) = spec.and_then(|spec| spec.resources.as_ref()) {
        for (name, amount) in amounts(resources.requests.as_ref()) {
            if supported_pod_level(&name) {
                requests.insert(name, amount);
            }
        }
        for (name, amount) in amounts(resources.limits.as_ref()) {
            if supported_pod_level(&name) {
                limits.insert(name, amount);
            }
        }
    }
    let overhead = amounts(spec.and_then(|spec| spec.overhead.as_ref()));
    add_amounts(&mut requests, &overhead);
    for (name, amount) in overhead {
        if let Some(limit) = limits.get_mut(&name).filter(|limit| limit.nanos != 0) {
            limit.nanos = limit.nanos.saturating_add(amount.nanos);
        }
    }
    (requests, limits)
}

fn value_or_zero(resources: &ResourceAmounts, name: &str) -> Amount {
    resources.get(name).copied().unwrap_or_default()
}

fn percent(used: Amount, total: Option<&Amount>, nanos_per_unit: i128) -> i128 {
    let used = used.rounded_units(nanos_per_unit);
    let total = total
        .copied()
        .unwrap_or_default()
        .rounded_units(nanos_per_unit);
    if total <= 0 {
        0
    } else {
        used.saturating_mul(100) / total
    }
}

fn resource_cell(amount: Amount, allocatable: Option<&Amount>, nanos_per_unit: i128) -> String {
    format!(
        "{} ({}%)",
        amount.canonical(),
        percent(amount, allocatable, nanos_per_unit)
    )
}

fn roles(node: &Node) -> String {
    let mut roles = BTreeSet::new();
    for (key, value) in node.metadata.labels.iter().flatten() {
        if let Some(role) = key
            .strip_prefix(ROLE_PREFIX)
            .filter(|role| !role.is_empty())
        {
            roles.insert(role);
        } else if key == LEGACY_ROLE_LABEL && !value.is_empty() {
            roles.insert(value.as_str());
        }
    }
    if roles.is_empty() {
        NONE.to_string()
    } else {
        roles.into_iter().collect::<Vec<_>>().join(",")
    }
}

fn lease_time(time: Option<&MicroTime>) -> String {
    time.map(|time| rfc1123z(&Time(time.0)))
        .unwrap_or_else(|| UNSET.to_string())
}

/// kubectl only emits this block when the heartbeat lease could be read.
fn write_lease(w: &mut Writer, lease: &Lease) {
    let spec = lease.spec.as_ref();
    w.section(0, "Lease");
    w.kv(
        1,
        "HolderIdentity",
        spec.and_then(|spec| spec.holder_identity.as_deref())
            .unwrap_or(UNSET),
    );
    w.kv(
        1,
        "AcquireTime",
        lease_time(spec.and_then(|spec| spec.acquire_time.as_ref())),
    );
    w.kv(
        1,
        "RenewTime",
        lease_time(spec.and_then(|spec| spec.renew_time.as_ref())),
    );
}

pub fn write(w: &mut Writer, node: &Node, lease: Option<&Lease>, pods: &[Pod], now_ms: i64) {
    let meta = &node.metadata;
    let spec = node.spec.as_ref();
    let status = node.status.as_ref();
    w.kv(0, "Name", or_none(meta.name.as_deref()));
    w.kv(0, "Roles", roles(node));
    write_labels_annotations(w, meta);
    if let Some(t) = &meta.creation_timestamp {
        w.kv(0, "CreationTimestamp", rfc1123z(t));
    }
    let taints: Vec<String> = spec
        .and_then(|s| s.taints.as_deref())
        .unwrap_or(&[])
        .iter()
        .map(|t| match t.value.as_deref().filter(|v| !v.is_empty()) {
            Some(v) => format!("{}={v}:{}", t.key, t.effect),
            None => format!("{}:{}", t.key, t.effect),
        })
        .collect();
    write_list(w, 0, "Taints", &taints);
    w.kv(
        0,
        "Unschedulable",
        spec.and_then(|s| s.unschedulable).unwrap_or(false),
    );
    if let Some(lease) = lease {
        write_lease(w, lease);
    }
    if let Some(conds) = status
        .and_then(|s| s.conditions.as_ref())
        .filter(|c| !c.is_empty())
    {
        w.section(0, "Conditions");
        w.cells(
            1,
            &[
                "Type",
                "Status",
                "LastHeartbeatTime",
                "LastTransitionTime",
                "Reason",
                "Message",
            ],
        );
        w.cells(
            1,
            &[
                "----",
                "------",
                "-----------------",
                "------------------",
                "------",
                "-------",
            ],
        );
        for c in conds {
            let hb = c
                .last_heartbeat_time
                .as_ref()
                .map(rfc1123z)
                .unwrap_or_default();
            let tt = c
                .last_transition_time
                .as_ref()
                .map(rfc1123z)
                .unwrap_or_default();
            w.cells(
                1,
                &[
                    &c.type_,
                    &c.status,
                    &hb,
                    &tt,
                    c.reason.as_deref().unwrap_or(""),
                    c.message.as_deref().unwrap_or(""),
                ],
            );
        }
    }
    w.section(0, "Addresses");
    for address in status
        .and_then(|status| status.addresses.as_ref())
        .into_iter()
        .flatten()
    {
        w.kv(1, &address.type_, &address.address);
    }
    write_quantities(w, 0, "Capacity", status.and_then(|s| s.capacity.as_ref()));
    write_quantities(
        w,
        0,
        "Allocatable",
        status.and_then(|s| s.allocatable.as_ref()),
    );
    let info = status.and_then(|status| status.node_info.as_ref());
    w.section(0, "System Info");
    w.kv(1, "Machine ID", info.map_or("", |info| &info.machine_id));
    w.kv(1, "System UUID", info.map_or("", |info| &info.system_uuid));
    w.kv(1, "Boot ID", info.map_or("", |info| &info.boot_id));
    w.kv(
        1,
        "Kernel Version",
        info.map_or("", |info| &info.kernel_version),
    );
    w.kv(1, "OS Image", info.map_or("", |info| &info.os_image));
    w.kv(
        1,
        "Operating System",
        info.map_or("", |info| &info.operating_system),
    );
    w.kv(
        1,
        "Architecture",
        info.map_or("", |info| &info.architecture),
    );
    w.kv(
        1,
        "Container Runtime Version",
        info.map_or("", |info| &info.container_runtime_version),
    );
    w.kv(
        1,
        "Kubelet Version",
        info.map_or("", |info| &info.kubelet_version),
    );
    let kube_proxy_version = info.map_or("", |info| &info.kube_proxy_version);
    if !kube_proxy_version.is_empty() {
        w.kv(1, "Kube-Proxy Version", kube_proxy_version);
    }
    if let Some(c) = spec.and_then(|s| s.pod_cidr.as_deref()) {
        w.kv(0, "PodCIDR", c);
    }
    if let Some(cs) = spec
        .and_then(|s| s.pod_cidrs.as_ref())
        .filter(|c| !c.is_empty())
    {
        w.kv(0, "PodCIDRs", cs.join(","));
    }
    if let Some(provider_id) = spec.and_then(|spec| spec.provider_id.as_deref()) {
        if !provider_id.is_empty() {
            w.kv(0, "ProviderID", provider_id);
        }
    }
    let allocatable_quantities = status.and_then(|status| {
        status
            .allocatable
            .as_ref()
            .filter(|allocatable| !allocatable.is_empty())
            .or(status.capacity.as_ref())
    });
    let allocatable = amounts(allocatable_quantities);
    let running: Vec<&Pod> = pods
        .iter()
        .filter(|p| {
            !matches!(
                p.status.as_ref().and_then(|s| s.phase.as_deref()),
                Some("Succeeded") | Some("Failed")
            )
        })
        .collect();
    w.kv(
        0,
        "Non-terminated Pods",
        format!("({} in total)", running.len()),
    );
    w.cells(
        1,
        &[
            "Namespace",
            "Name",
            "CPU Requests",
            "CPU Limits",
            "Memory Requests",
            "Memory Limits",
            "Age",
        ],
    );
    w.cells(
        1,
        &[
            "---------",
            "----",
            "------------",
            "----------",
            "---------------",
            "-------------",
            "---",
        ],
    );
    let mut total_requests = ResourceAmounts::new();
    let mut total_limits = ResourceAmounts::new();
    for p in &running {
        let (requests, limits) = pod_resource_amounts(p, true);
        add_amounts(&mut total_requests, &requests);
        add_amounts(&mut total_limits, &limits);
        let created = p
            .metadata
            .creation_timestamp
            .as_ref()
            .map(|t| t.0.to_rfc3339());
        let age = human_age(created.as_deref(), now_ms);
        let cpu_request = resource_cell(
            value_or_zero(&requests, "cpu"),
            allocatable.get("cpu"),
            1_000_000,
        );
        let cpu_limit = resource_cell(
            value_or_zero(&limits, "cpu"),
            allocatable.get("cpu"),
            1_000_000,
        );
        let memory_request = resource_cell(
            value_or_zero(&requests, "memory"),
            allocatable.get("memory"),
            NANO,
        );
        let memory_limit = resource_cell(
            value_or_zero(&limits, "memory"),
            allocatable.get("memory"),
            NANO,
        );
        w.cells(
            1,
            &[
                p.metadata.namespace.as_deref().unwrap_or(""),
                p.metadata.name.as_deref().unwrap_or(""),
                &cpu_request,
                &cpu_limit,
                &memory_request,
                &memory_limit,
                &age,
            ],
        );
    }
    w.section(0, "Allocated resources");
    w.text(
        1,
        "(Total limits may be over 100 percent, i.e., overcommitted.)",
    );
    w.cells(1, &["Resource", "Requests", "Limits"]);
    w.cells(1, &["--------", "--------", "------"]);
    let cpu_request = resource_cell(
        value_or_zero(&total_requests, "cpu"),
        allocatable.get("cpu"),
        1_000_000,
    );
    let cpu_limit = resource_cell(
        value_or_zero(&total_limits, "cpu"),
        allocatable.get("cpu"),
        1_000_000,
    );
    w.cells(1, &["cpu", &cpu_request, &cpu_limit]);
    let memory_request = resource_cell(
        value_or_zero(&total_requests, "memory"),
        allocatable.get("memory"),
        NANO,
    );
    let memory_limit = resource_cell(
        value_or_zero(&total_limits, "memory"),
        allocatable.get("memory"),
        NANO,
    );
    w.cells(1, &["memory", &memory_request, &memory_limit]);
    let ephemeral_request = resource_cell(
        value_or_zero(&total_requests, "ephemeral-storage"),
        allocatable.get("ephemeral-storage"),
        NANO,
    );
    let ephemeral_limit = resource_cell(
        value_or_zero(&total_limits, "ephemeral-storage"),
        allocatable.get("ephemeral-storage"),
        NANO,
    );
    w.cells(
        1,
        &["ephemeral-storage", &ephemeral_request, &ephemeral_limit],
    );
    for name in allocatable
        .keys()
        .filter(|name| name.starts_with("hugepages-"))
    {
        let request = resource_cell(
            value_or_zero(&total_requests, name),
            allocatable.get(name),
            NANO,
        );
        let limit = resource_cell(
            value_or_zero(&total_limits, name),
            allocatable.get(name),
            NANO,
        );
        w.cells(1, &[name, &request, &limit]);
    }
    for name in allocatable.keys().filter(|name| {
        !matches!(
            name.as_str(),
            "cpu" | "memory" | "ephemeral-storage" | "pods"
        ) && !name.starts_with("hugepages-")
    }) {
        let request = value_or_zero(&total_requests, name).canonical();
        let limit = value_or_zero(&total_limits, name).canonical();
        w.cells(1, &[name, &request, &limit]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        NodeCondition, NodeSpec, NodeStatus, PodCondition, PodSpec, PodStatus, ResourceRequirements,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn quantities(values: &[(&str, &str)]) -> BTreeMap<String, Quantity> {
        values
            .iter()
            .map(|(name, value)| (name.to_string(), Quantity(value.to_string())))
            .collect()
    }

    fn container(name: &str, requests: &[(&str, &str)], limits: &[(&str, &str)]) -> Container {
        Container {
            name: name.to_string(),
            resources: Some(ResourceRequirements {
                requests: Some(quantities(requests)),
                limits: Some(quantities(limits)),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn parsed(value: &str) -> Amount {
        Amount::parse(&Quantity(value.to_string())).unwrap()
    }

    #[test]
    fn quantities_add_exactly_and_format_canonically() {
        assert_eq!(parsed("100m").canonical(), "100m");
        assert_eq!(parsed("2").canonical(), "2");
        assert_eq!(parsed("1.5").canonical(), "1500m");
        assert_eq!(parsed("64Mi").canonical(), "64Mi");
        assert_eq!(parsed("1.5Gi").canonical(), "1536Mi");
        assert_eq!(parsed("1e3").canonical(), "1e3");
        assert_eq!(parsed("1.5e3").canonical(), "1500");

        let mut values = ResourceAmounts::new();
        add_amounts(&mut values, &amounts(Some(&quantities(&[("cpu", "0.1")]))));
        add_amounts(&mut values, &amounts(Some(&quantities(&[("cpu", "0.2")]))));
        assert_eq!(value_or_zero(&values, "cpu").canonical(), "300m");

        let mut exponent_values = amounts(Some(&quantities(&[("widgets", "1e3")])));
        add_amounts(
            &mut exponent_values,
            &amounts(Some(&quantities(&[("widgets", "2e3")]))),
        );
        assert_eq!(
            value_or_zero(&exponent_values, "widgets").canonical(),
            "3e3"
        );

        assert!(Amount::parse(&Quantity(format!("{}E", i128::MAX))).is_none());
    }

    #[test]
    fn percentages_truncate_and_handle_missing_or_zero_allocatable() {
        assert_eq!(
            resource_cell(parsed("100m"), Some(&parsed("4")), 1_000_000),
            "100m (2%)"
        );
        assert_eq!(
            resource_cell(parsed("500m"), Some(&parsed("4")), 1_000_000),
            "500m (12%)"
        );
        assert_eq!(resource_cell(parsed("100m"), None, 1_000_000), "100m (0%)");
        assert_eq!(
            resource_cell(parsed("100m"), Some(&parsed("0")), 1_000_000),
            "100m (0%)"
        );
    }

    #[test]
    fn pod_resources_include_init_sidecars_and_overhead() {
        let pod = Pod {
            spec: Some(PodSpec {
                containers: vec![container(
                    "app",
                    &[("cpu", "100m"), ("memory", "100Mi")],
                    &[("cpu", "200m"), ("memory", "200Mi")],
                )],
                init_containers: Some(vec![
                    Container {
                        restart_policy: Some("Always".to_string()),
                        ..container(
                            "sidecar",
                            &[("cpu", "50m"), ("memory", "50Mi")],
                            &[("cpu", "100m"), ("memory", "50Mi")],
                        )
                    },
                    container(
                        "init",
                        &[("cpu", "500m"), ("memory", "20Mi")],
                        &[("cpu", "600m"), ("memory", "20Mi")],
                    ),
                ]),
                overhead: Some(quantities(&[("cpu", "10m"), ("memory", "1Mi")])),
                ..Default::default()
            }),
            status: Some(PodStatus {
                init_container_statuses: Some(vec![ContainerStatus {
                    name: "init".to_string(),
                    allocated_resources: Some(quantities(&[("cpu", "2")])),
                    resources: Some(ResourceRequirements {
                        requests: Some(quantities(&[("cpu", "2")])),
                        limits: Some(quantities(&[("cpu", "2")])),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let (requests, limits) = pod_resource_amounts(&pod, true);
        assert_eq!(value_or_zero(&requests, "cpu").canonical(), "560m");
        assert_eq!(value_or_zero(&requests, "memory").canonical(), "151Mi");
        assert_eq!(value_or_zero(&limits, "cpu").canonical(), "710m");
        assert_eq!(value_or_zero(&limits, "memory").canonical(), "251Mi");
    }

    #[test]
    fn pod_level_resources_override_supported_container_totals() {
        let pod = Pod {
            spec: Some(PodSpec {
                containers: vec![container("app", &[("cpu", "100m")], &[("cpu", "200m")])],
                resources: Some(ResourceRequirements {
                    requests: Some(quantities(&[("cpu", "750m")])),
                    limits: Some(quantities(&[("cpu", "1")])),
                    ..Default::default()
                }),
                overhead: Some(quantities(&[("cpu", "10m")])),
                ..Default::default()
            }),
            ..Default::default()
        };

        let (requests, limits) = pod_resource_amounts(&pod, false);
        assert_eq!(value_or_zero(&requests, "cpu").canonical(), "760m");
        assert_eq!(value_or_zero(&limits, "cpu").canonical(), "1010m");
    }

    #[test]
    fn status_resources_follow_in_place_resize_semantics() {
        let mut pod = Pod {
            spec: Some(PodSpec {
                containers: vec![container("app", &[("cpu", "100m")], &[("cpu", "500m")])],
                ..Default::default()
            }),
            status: Some(PodStatus {
                container_statuses: Some(vec![ContainerStatus {
                    name: "app".to_string(),
                    allocated_resources: Some(quantities(&[("cpu", "300m")])),
                    resources: Some(ResourceRequirements {
                        requests: Some(quantities(&[("cpu", "200m")])),
                        limits: Some(quantities(&[("cpu", "400m")])),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let (requests, limits) = pod_resource_amounts(&pod, true);
        assert_eq!(value_or_zero(&requests, "cpu").canonical(), "300m");
        assert_eq!(value_or_zero(&limits, "cpu").canonical(), "500m");

        pod.status.as_mut().unwrap().conditions = Some(vec![PodCondition {
            type_: "PodResizePending".to_string(),
            reason: Some("Infeasible".to_string()),
            ..Default::default()
        }]);
        let (requests, limits) = pod_resource_amounts(&pod, true);
        assert_eq!(value_or_zero(&requests, "cpu").canonical(), "300m");
        assert_eq!(value_or_zero(&limits, "cpu").canonical(), "500m");
    }

    #[test]
    fn rendered_rows_and_totals_use_the_same_status_resources() {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("resizing".to_string()),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![container("app", &[("cpu", "100m")], &[("cpu", "500m")])],
                ..Default::default()
            }),
            status: Some(PodStatus {
                container_statuses: Some(vec![ContainerStatus {
                    name: "app".to_string(),
                    allocated_resources: Some(quantities(&[("cpu", "300m")])),
                    resources: Some(ResourceRequirements {
                        requests: Some(quantities(&[("cpu", "200m")])),
                        limits: Some(quantities(&[("cpu", "400m")])),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        };
        let node = Node {
            status: Some(NodeStatus {
                allocatable: Some(quantities(&[("cpu", "1")])),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut w = Writer::new();
        write(&mut w, &node, None, &[pod], 0);
        let matching_rows = w
            .finish()
            .lines()
            .filter(|line| {
                let cells: Vec<_> = line.split_whitespace().collect();
                cells
                    .windows(4)
                    .any(|cells| cells == ["300m", "(30%)", "500m", "(50%)"])
            })
            .count();

        assert_eq!(matching_rows, 2);
    }

    #[test]
    fn node_filters_terminal_pods() {
        let pod = |name: &str, phase: &str| Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: Some(phase.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut w = Writer::new();
        write(
            &mut w,
            &Node::default(),
            None,
            &[
                pod("running", "Running"),
                pod("done", "Succeeded"),
                pod("failed", "Failed"),
            ],
            0,
        );
        let output = w.finish();

        assert!(output.contains("(1 in total)"));
        assert!(output.contains("running"));
        assert!(!output.contains("done"));
        assert!(!output.contains("failed"));
    }

    #[test]
    fn node_uses_capacity_fallback_and_lists_extra_resources_in_order() {
        let pod = Pod {
            spec: Some(PodSpec {
                containers: vec![container(
                    "app",
                    &[
                        ("cpu", "100m"),
                        ("memory", "256Mi"),
                        ("ephemeral-storage", "1Gi"),
                        ("hugepages-2Mi", "2Mi"),
                        ("example.com/gpu", "1"),
                    ],
                    &[
                        ("cpu", "200m"),
                        ("memory", "512Mi"),
                        ("ephemeral-storage", "2Gi"),
                        ("hugepages-2Mi", "2Mi"),
                        ("example.com/gpu", "1"),
                    ],
                )],
                ..Default::default()
            }),
            ..Default::default()
        };
        let node = Node {
            metadata: ObjectMeta {
                labels: Some(BTreeMap::from([
                    (
                        "node-role.kubernetes.io/control-plane".to_string(),
                        "".to_string(),
                    ),
                    (LEGACY_ROLE_LABEL.to_string(), "worker".to_string()),
                ])),
                ..Default::default()
            },
            spec: Some(NodeSpec {
                provider_id: Some("kind://docker/node-a".to_string()),
                ..Default::default()
            }),
            status: Some(NodeStatus {
                capacity: Some(quantities(&[
                    ("cpu", "0"),
                    ("memory", "1Gi"),
                    ("ephemeral-storage", "10Gi"),
                    ("hugepages-2Mi", "4Mi"),
                    ("example.com/gpu", "2"),
                    ("aaa.com/fpga", "4"),
                ])),
                allocatable: Some(BTreeMap::new()),
                ..Default::default()
            }),
        };
        let mut w = Writer::new();
        write(&mut w, &node, None, &[pod], 0);
        let output = w.finish();

        assert!(output.contains("control-plane,worker"));
        assert!(output.contains("kind://docker/node-a"));
        let allocated = output.split("Allocated resources:\n").nth(1).unwrap();
        assert!(allocated.lines().any(|line| {
            line.split_whitespace().collect::<Vec<_>>() == ["cpu", "100m", "(0%)", "200m", "(0%)"]
        }));
        assert!(allocated.lines().any(|line| {
            line.split_whitespace().collect::<Vec<_>>()
                == ["memory", "256Mi", "(25%)", "512Mi", "(50%)"]
        }));
        assert!(allocated.lines().any(|line| {
            line.split_whitespace().collect::<Vec<_>>()
                == ["ephemeral-storage", "1Gi", "(10%)", "2Gi", "(20%)"]
        }));
        assert!(allocated.lines().any(|line| {
            line.split_whitespace().collect::<Vec<_>>()
                == ["hugepages-2Mi", "2Mi", "(50%)", "2Mi", "(50%)"]
        }));
        let hugepages = allocated.find("hugepages-2Mi").unwrap();
        let fpga = allocated.find("aaa.com/fpga").unwrap();
        let gpu = allocated.find("example.com/gpu").unwrap();
        assert!(hugepages < fpga && fpga < gpu);
    }

    #[test]
    fn empty_node_still_prints_addresses_and_system_info() {
        let mut w = Writer::new();
        write(&mut w, &Node::default(), None, &[], 0);
        let output = w.finish();

        assert!(output.contains("Addresses:\n"));
        assert!(output.contains("System Info:\n"));
        assert!(output.contains("Machine ID:"));
    }

    /// Exact bytes, so the column widths of the three node tables are pinned.
    #[test]
    fn node_tables_are_column_aligned() {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("web-1".to_string()),
                namespace: Some("default".to_string()),
                creation_timestamp: Some(Time("2026-07-01T00:00:00Z".parse().unwrap())),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![container(
                    "web",
                    &[("cpu", "100m"), ("memory", "64Mi")],
                    &[("cpu", "500m"), ("memory", "128Mi")],
                )],
                ..Default::default()
            }),
            status: Some(PodStatus {
                phase: Some("Running".to_string()),
                ..Default::default()
            }),
        };
        let node = Node {
            status: Some(NodeStatus {
                allocatable: Some(quantities(&[("cpu", "2"), ("memory", "1Gi")])),
                conditions: Some(vec![NodeCondition {
                    type_: "Ready".to_string(),
                    status: "True".to_string(),
                    last_heartbeat_time: Some(Time("2026-07-03T11:59:00Z".parse().unwrap())),
                    last_transition_time: Some(Time("2026-07-01T00:00:00Z".parse().unwrap())),
                    reason: Some("KubeletReady".to_string()),
                    message: Some("kubelet is posting ready status".to_string()),
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut w = Writer::new();
        write(&mut w, &node, None, &[pod], 1_783_080_000_000);
        let output = w.finish();

        assert!(output.contains(concat!(
            "Conditions:\n",
            "  Type   Status  LastHeartbeatTime                LastTransitionTime               Reason        Message\n",
            "  ----   ------  -----------------                ------------------               ------        -------\n",
            "  Ready  True    Fri, 03 Jul 2026 11:59:00 +0000  Wed, 01 Jul 2026 00:00:00 +0000  KubeletReady  kubelet is posting ready status\n",
        )), "conditions table:\n{output}");
        // The System Info entries share the block, so column 0 is as wide as
        // `  Container Runtime Version:` — exactly what kubectl prints.
        assert!(output.contains(concat!(
            "Non-terminated Pods:          (1 in total)\n",
            "  Namespace                   Name   CPU Requests  CPU Limits  Memory Requests  Memory Limits  Age\n",
            "  ---------                   ----   ------------  ----------  ---------------  -------------  ---\n",
            "  default                     web-1  100m (5%)     500m (25%)  64Mi (6%)        128Mi (12%)    2d12h\n",
        )), "pod table:\n{output}");
        assert!(
            output.contains(concat!(
                "Allocated resources:\n",
                "  (Total limits may be over 100 percent, i.e., overcommitted.)\n",
                "  Resource           Requests   Limits\n",
                "  --------           --------   ------\n",
                "  cpu                100m (5%)  500m (25%)\n",
                "  memory             64Mi (6%)  128Mi (12%)\n",
                "  ephemeral-storage  0 (0%)     0 (0%)\n",
            )),
            "allocated resources table:\n{output}"
        );
    }

    #[test]
    fn missing_lease_omits_the_whole_block() {
        let mut w = Writer::new();
        write(&mut w, &Node::default(), None, &[], 0);
        let output = w.finish();

        assert!(!output.contains("Lease:"));
        assert!(!output.contains("HolderIdentity:"));
        assert!(!output.contains("<unset>"));
    }

    #[test]
    fn lease_fields_default_to_unset() {
        let mut w = Writer::new();
        write_lease(&mut w, &Lease::default());

        assert_eq!(
            w.finish(),
            "Lease:\n  HolderIdentity:  <unset>\n  AcquireTime:     <unset>\n  RenewTime:       <unset>\n"
        );
    }
}
