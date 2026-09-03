use super::header::{annotation_lines, write_controlled_by, write_labels_annotations};
use super::util::{
    bool_title, int_or_string, map_lines, or_none, rfc1123z, selector_string, terminating_status,
    write_list, write_quantities, NONE, UNKNOWN, UNSET,
};
use super::writer::Writer;
use k8s_openapi::api::core::v1::{
    Container, ContainerState, ContainerStatus, EnvFromSource, EphemeralContainer, Pod,
    PodTemplateSpec, Probe, Toleration, TopologySpreadConstraint, Volume,
};

/// kubectl's pod-template `space`: the section header moves down a level and
/// each container or volume name gains one extra space, keeping their fields at
/// kubectl's LEVEL_2.
const TEMPLATE_NAME_PREFIX: &str = " ";

pub fn write(w: &mut Writer, pod: &Pod, now_ms: i64) {
    let meta = &pod.metadata;
    let spec = pod.spec.as_ref();
    let status = pod.status.as_ref();
    w.kv(0, "Name", or_none(meta.name.as_deref()));
    w.kv(0, "Namespace", or_none(meta.namespace.as_deref()));
    if let Some(p) = spec.and_then(|s| s.priority) {
        w.kv(0, "Priority", p);
    }
    if let Some(pc) = spec.and_then(|s| s.priority_class_name.as_deref()) {
        w.kv(0, "Priority Class Name", pc);
    }
    w.kv(
        0,
        "Service Account",
        or_none(spec.and_then(|s| s.service_account_name.as_deref())),
    );
    let node = match (
        spec.and_then(|s| s.node_name.as_deref()),
        status.and_then(|s| s.host_ip.as_deref()),
    ) {
        (Some(n), Some(ip)) => format!("{n}/{ip}"),
        (Some(n), None) => n.to_string(),
        _ => NONE.to_string(),
    };
    w.kv(0, "Node", node);
    if let Some(t) = status.and_then(|s| s.start_time.as_ref()) {
        w.kv(0, "Start Time", rfc1123z(t));
    }
    write_labels_annotations(w, meta);
    let phase = status.and_then(|s| s.phase.as_deref());
    let terminating = meta
        .deletion_timestamp
        .as_ref()
        .filter(|_| !matches!(phase, Some("Failed") | Some("Succeeded")));
    match terminating {
        Some(deletion_timestamp) => {
            w.kv(0, "Status", terminating_status(deletion_timestamp, now_ms));
            w.kv(
                0,
                "Termination Grace Period",
                format!("{}s", meta.deletion_grace_period_seconds.unwrap_or(0)),
            );
        }
        None => w.kv(0, "Status", or_none(phase)),
    }
    if let Some(r) = status.and_then(|s| s.reason.as_deref()) {
        w.kv(0, "Reason", r);
    }
    if let Some(m) = status.and_then(|s| s.message.as_deref()) {
        w.kv(0, "Message", m);
    }
    w.kv(0, "IP", or_none(status.and_then(|s| s.pod_ip.as_deref())));
    match status
        .and_then(|s| s.pod_ips.as_deref())
        .filter(|ips| !ips.is_empty())
    {
        Some(ips) => {
            w.section(0, "IPs");
            for ip in ips {
                w.kv(1, "IP", &ip.ip);
            }
        }
        None => w.kv(0, "IPs", NONE),
    }
    write_controlled_by(w, meta);
    if let Some(s) = spec {
        if let Some(ic) = s.init_containers.as_deref().filter(|v| !v.is_empty()) {
            write_containers(
                w,
                0,
                "",
                "Init Containers",
                ic,
                status.and_then(|st| st.init_container_statuses.as_deref()),
                Some(pod),
            );
        }
        write_containers(
            w,
            0,
            "",
            "Containers",
            &s.containers,
            status.and_then(|st| st.container_statuses.as_deref()),
            Some(pod),
        );
        if let Some(ec) = s.ephemeral_containers.as_deref().filter(|v| !v.is_empty()) {
            let containers: Vec<Container> = ec.iter().map(ephemeral_as_container).collect();
            write_containers(
                w,
                0,
                "",
                "Ephemeral Containers",
                &containers,
                status.and_then(|st| st.ephemeral_container_statuses.as_deref()),
                Some(pod),
            );
        }
    }
    if let Some(conds) = status
        .and_then(|s| s.conditions.as_ref())
        .filter(|c| !c.is_empty())
    {
        w.section(0, "Conditions");
        w.cells(1, &["Type", "Status"]);
        for c in conds {
            w.cells(1, &[&c.type_, &c.status]);
        }
    }
    write_volumes(w, 0, "", spec.and_then(|s| s.volumes.as_deref()));
    w.kv(
        0,
        "QoS Class",
        or_none(status.and_then(|s| s.qos_class.as_deref())),
    );
    write_list(
        w,
        0,
        "Node-Selectors",
        &map_lines(spec.and_then(|s| s.node_selector.as_ref())),
    );
    write_list(
        w,
        0,
        "Tolerations",
        &toleration_lines(spec.and_then(|s| s.tolerations.as_deref())),
    );
    write_topology_spread_constraints(
        w,
        0,
        spec.and_then(|s| s.topology_spread_constraints.as_deref()),
    );
}

/// kubectl converts each `EphemeralContainerCommon` to a `Container` before
/// printing it; the two field sets differ only in `targetContainerName`.
fn ephemeral_as_container(ec: &EphemeralContainer) -> Container {
    serde_json::to_value(ec)
        .and_then(serde_json::from_value)
        .unwrap_or_else(|_| Container {
            name: ec.name.clone(),
            image: ec.image.clone(),
            ..Default::default()
        })
}

/// `Containers:` / `Init Containers:` section; statuses are matched by name.
/// `name_prefix` is kubectl's pod-template `space`: when it is set the header
/// keeps `level` and each container name sits at `level` plus that prefix
/// rather than a whole level deeper.
pub fn write_containers(
    w: &mut Writer,
    level: usize,
    name_prefix: &str,
    key: &str,
    containers: &[Container],
    statuses: Option<&[ContainerStatus]>,
    pod: Option<&Pod>,
) {
    let name_level = name_level(level, name_prefix);
    w.section(level, key);
    for c in containers {
        let st = statuses.and_then(|s| s.iter().find(|s| s.name == c.name));
        write_container(w, name_level, name_prefix, c, st, pod);
    }
}

fn name_level(level: usize, name_prefix: &str) -> usize {
    if name_prefix.is_empty() {
        level + 1
    } else {
        level
    }
}

pub fn write_container(
    w: &mut Writer,
    level: usize,
    name_prefix: &str,
    c: &Container,
    st: Option<&ContainerStatus>,
    pod: Option<&Pod>,
) {
    w.section(level, &format!("{name_prefix}{}", c.name));
    let i = level + 1;
    if let Some(s) = st {
        w.kv(i, "Container ID", s.container_id.as_deref().unwrap_or(""));
    }
    w.kv(i, "Image", or_none(c.image.as_deref()));
    if let Some(s) = st {
        w.kv(i, "Image ID", &s.image_id);
    }
    let ports = c.ports.as_deref().unwrap_or(&[]);
    let port_strs: Vec<String> = ports
        .iter()
        .map(|p| {
            let port = format!(
                "{}/{}",
                p.container_port,
                p.protocol.as_deref().unwrap_or("TCP")
            );
            match p.name.as_deref().filter(|name| !name.is_empty()) {
                Some(name) => format!("{port} ({name})"),
                None => port,
            }
        })
        .collect();
    let host_strs: Vec<String> = ports
        .iter()
        .map(|p| {
            let port = format!(
                "{}/{}",
                p.host_port.unwrap_or(0),
                p.protocol.as_deref().unwrap_or("TCP")
            );
            match p.name.as_deref().filter(|name| !name.is_empty()) {
                Some(name) => format!("{port} ({name})"),
                None => port,
            }
        })
        .collect();
    let (pk, hk) = if ports.len() > 1 {
        ("Ports", "Host Ports")
    } else {
        ("Port", "Host Port")
    };
    w.kv(
        i,
        pk,
        if port_strs.is_empty() {
            NONE.to_string()
        } else {
            port_strs.join(", ")
        },
    );
    w.kv(
        i,
        hk,
        if host_strs.is_empty() {
            NONE.to_string()
        } else {
            host_strs.join(", ")
        },
    );
    if let Some(cmd) = c.command.as_ref().filter(|v| !v.is_empty()) {
        w.section(i, "Command");
        for a in cmd {
            for line in a.split('\n') {
                w.text(i + 1, line);
            }
        }
    }
    if let Some(args) = c.args.as_ref().filter(|v| !v.is_empty()) {
        w.section(i, "Args");
        for a in args {
            for line in a.split('\n') {
                w.text(i + 1, line);
            }
        }
    }
    if let Some(s) = st {
        write_state(w, i, "State", s.state.as_ref());
        if let Some(last) = s
            .last_state
            .as_ref()
            .filter(|l| l.running.is_some() || l.waiting.is_some() || l.terminated.is_some())
        {
            write_state(w, i, "Last State", Some(last));
        }
        w.kv(i, "Ready", bool_title(s.ready));
        w.kv(i, "Restart Count", s.restart_count);
    }
    write_quantities(
        w,
        i,
        "Limits",
        c.resources.as_ref().and_then(|r| r.limits.as_ref()),
    );
    write_quantities(
        w,
        i,
        "Requests",
        c.resources.as_ref().and_then(|r| r.requests.as_ref()),
    );
    if let Some(p) = &c.liveness_probe {
        w.kv(i, "Liveness", probe_string(p));
    }
    if let Some(p) = &c.readiness_probe {
        w.kv(i, "Readiness", probe_string(p));
    }
    if let Some(p) = &c.startup_probe {
        w.kv(i, "Startup", probe_string(p));
    }
    write_env_from(w, i, c.env_from.as_deref().unwrap_or(&[]));
    write_env(w, i, c, pod);
    let mounts = c.volume_mounts.as_deref().unwrap_or(&[]);
    if mounts.is_empty() {
        w.kv(i, "Mounts", NONE);
    } else {
        w.section(i, "Mounts");
        let mut mounts: Vec<_> = mounts.iter().collect();
        mounts.sort_by(|a, b| a.mount_path.cmp(&b.mount_path));
        for m in mounts {
            let mode = if m.read_only.unwrap_or(false) {
                "ro"
            } else {
                "rw"
            };
            let sub = m
                .sub_path
                .as_deref()
                .map(|s| format!(",path=\"{s}\""))
                .unwrap_or_default();
            w.text(
                i + 1,
                &format!("{} from {} ({mode}{sub})", m.mount_path, m.name),
            );
        }
    }
}

fn write_state(w: &mut Writer, level: usize, key: &str, state: Option<&ContainerState>) {
    let Some(s) = state else {
        w.kv(level, key, "Waiting");
        return;
    };
    if let Some(r) = &s.running {
        w.kv(level, key, "Running");
        if let Some(t) = &r.started_at {
            w.kv(level + 1, "Started", rfc1123z(t));
        }
    } else if let Some(t) = &s.terminated {
        w.kv(level, key, "Terminated");
        if let Some(r) = &t.reason {
            w.kv(level + 1, "Reason", r);
        }
        if let Some(m) = &t.message {
            w.kv(level + 1, "Message", m);
        }
        w.kv(level + 1, "Exit Code", t.exit_code);
        if let Some(sig) = t.signal {
            w.kv(level + 1, "Signal", sig);
        }
        if let Some(st) = &t.started_at {
            w.kv(level + 1, "Started", rfc1123z(st));
        }
        if let Some(f) = &t.finished_at {
            w.kv(level + 1, "Finished", rfc1123z(f));
        }
    } else {
        w.kv(level, key, "Waiting");
        if let Some(wt) = &s.waiting {
            if let Some(r) = &wt.reason {
                w.kv(level + 1, "Reason", r);
            }
            if let Some(m) = &wt.message {
                w.kv(level + 1, "Message", m);
            }
        }
    }
}

/// kubectl probe line: `http-get http://:80/healthz delay=0s timeout=1s period=10s #success=1 #failure=3`.
pub fn probe_string(p: &Probe) -> String {
    let attrs = format!(
        "delay={}s timeout={}s period={}s #success={} #failure={}",
        p.initial_delay_seconds.unwrap_or(0),
        p.timeout_seconds.unwrap_or(1),
        p.period_seconds.unwrap_or(10),
        p.success_threshold.unwrap_or(1),
        p.failure_threshold.unwrap_or(3)
    );
    let handler = if let Some(e) = &p.exec {
        format!("exec [{}]", e.command.as_deref().unwrap_or(&[]).join(" "))
    } else if let Some(h) = &p.http_get {
        format!(
            "http-get {}://{}:{}{}",
            h.scheme.as_deref().unwrap_or("HTTP").to_lowercase(),
            h.host.as_deref().unwrap_or(""),
            int_or_string(&h.port),
            h.path.as_deref().unwrap_or("")
        )
    } else if let Some(t) = &p.tcp_socket {
        format!(
            "tcp-socket {}:{}",
            t.host.as_deref().unwrap_or(""),
            int_or_string(&t.port)
        )
    } else if let Some(g) = &p.grpc {
        format!(
            "grpc <pod>:{} {}",
            g.port,
            g.service.as_deref().unwrap_or("")
        )
    } else {
        "unknown".to_string()
    };
    format!("{handler} {attrs}")
}

fn write_multiline_value(w: &mut Writer, level: usize, key: &str, value: &str) {
    let mut lines = value.split('\n');
    w.kv(level, key, lines.next().unwrap_or_default());
    for line in lines {
        w.cont(level, line);
    }
}

fn pod_field_value(pod: Option<&Pod>, field_path: &str) -> String {
    let Some(pod) = pod else {
        return String::new();
    };
    if let Some(path) = field_path.strip_suffix("']") {
        if let Some((map, key)) = path.split_once("['") {
            return match map {
                "metadata.annotations" => pod
                    .metadata
                    .annotations
                    .as_ref()
                    .and_then(|values| values.get(key))
                    .cloned()
                    .unwrap_or_default(),
                "metadata.labels" => pod
                    .metadata
                    .labels
                    .as_ref()
                    .and_then(|values| values.get(key))
                    .cloned()
                    .unwrap_or_default(),
                _ => String::new(),
            };
        }
    }
    match field_path {
        "metadata.annotations" => pod
            .metadata
            .annotations
            .as_ref()
            .map(|values| {
                values
                    .iter()
                    .map(|(key, value)| format!("{key}={value:?}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        "metadata.labels" => pod
            .metadata
            .labels
            .as_ref()
            .map(|values| {
                values
                    .iter()
                    .map(|(key, value)| format!("{key}={value:?}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        "metadata.name" => pod.metadata.name.clone().unwrap_or_default(),
        "metadata.namespace" => pod.metadata.namespace.clone().unwrap_or_default(),
        "metadata.uid" => pod.metadata.uid.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

fn container_resource_value(
    container: &Container,
    selector: &k8s_openapi::api::core::v1::ResourceFieldSelector,
) -> String {
    let Some((scope, name)) = selector.resource.split_once('.') else {
        return String::new();
    };
    if !matches!(scope, "limits" | "requests")
        || !matches!(name, "cpu" | "memory" | "ephemeral-storage")
            && !name.starts_with("hugepages-")
    {
        return String::new();
    }
    let quantity = container
        .resources
        .as_ref()
        .and_then(|resources| match scope {
            "limits" => resources.limits.as_ref(),
            "requests" => resources.requests.as_ref(),
            _ => None,
        })
        .and_then(|values| values.get(name))
        .and_then(|quantity| crate::quantity::parse_quantity(&quantity.0))
        .unwrap_or(0.0);
    let divisor = selector
        .divisor
        .as_ref()
        .and_then(|quantity| crate::quantity::parse_quantity(&quantity.0))
        .filter(|divisor| *divisor != 0.0)
        .unwrap_or(1.0);
    let value = (quantity / divisor).ceil();
    if value == 0.0 && matches!(selector.resource.as_str(), "limits.cpu" | "limits.memory") {
        "node allocatable".to_string()
    } else if value.is_finite() {
        format!("{value:.0}")
    } else {
        String::new()
    }
}

fn write_env(w: &mut Writer, level: usize, c: &Container, pod: Option<&Pod>) {
    let env = c.env.as_deref().unwrap_or(&[]);
    if env.is_empty() {
        w.kv(level, "Environment", NONE);
        return;
    }
    w.section(level, "Environment");
    for e in env {
        match (&e.value, &e.value_from) {
            (Some(v), _) => write_multiline_value(w, level + 1, &e.name, v),
            (None, Some(from)) => {
                if let Some(f) = &from.field_ref {
                    w.kv(
                        level + 1,
                        &e.name,
                        format!(
                            "{} ({}:{})",
                            pod_field_value(pod, &f.field_path),
                            f.api_version.as_deref().unwrap_or("v1"),
                            f.field_path
                        ),
                    );
                } else if let Some(r) = &from.resource_field_ref {
                    w.kv(
                        level + 1,
                        &e.name,
                        format!("{} ({})", container_resource_value(c, r), r.resource),
                    );
                } else if let Some(s) = &from.secret_key_ref {
                    let key = format!("{}:", e.name);
                    let v = format!("<set to the key '{}' in secret '{}'>", s.key, s.name);
                    let o = format!("Optional: {}", s.optional.unwrap_or(false));
                    w.cells(level + 1, &[&key, &v, &o]);
                } else if let Some(c) = &from.config_map_key_ref {
                    let key = format!("{}:", e.name);
                    let v = format!("<set to the key '{}' of config map '{}'>", c.key, c.name);
                    let o = format!("Optional: {}", c.optional.unwrap_or(false));
                    w.cells(level + 1, &[&key, &v, &o]);
                } else {
                    w.kv(level + 1, &e.name, UNKNOWN);
                }
            }
            (None, None) => w.kv(level + 1, &e.name, ""),
        }
    }
}

fn write_env_from(w: &mut Writer, level: usize, env_from: &[EnvFromSource]) {
    if env_from.is_empty() {
        return;
    }
    w.section(level, "Environment Variables from");
    for e in env_from {
        let (name, kind, optional) = if let Some(c) = &e.config_map_ref {
            (c.name.as_str(), "ConfigMap", c.optional.unwrap_or(false))
        } else if let Some(s) = &e.secret_ref {
            (s.name.as_str(), "Secret", s.optional.unwrap_or(false))
        } else {
            continue;
        };
        let source = e
            .prefix
            .as_deref()
            .filter(|prefix| !prefix.is_empty())
            .map_or_else(
                || kind.to_string(),
                |prefix| format!("{kind} with prefix '{prefix}'"),
            );
        let tail = format!("Optional: {optional}");
        w.cells(level + 1, &[name, &source, &tail]);
    }
}

pub fn write_volumes(w: &mut Writer, level: usize, name_prefix: &str, volumes: Option<&[Volume]>) {
    let Some(vols) = volumes.filter(|v| !v.is_empty()) else {
        w.kv(level, "Volumes", NONE);
        return;
    };
    let name_level = name_level(level, name_prefix);
    w.section(level, "Volumes");
    for v in vols {
        w.section(name_level, &format!("{name_prefix}{}", v.name));
        let i = name_level + 1;
        if let Some(c) = &v.config_map {
            w.kv(i, "Type", "ConfigMap (a volume populated by a ConfigMap)");
            w.kv(i, "Name", &c.name);
            w.kv(i, "Optional", c.optional.unwrap_or(false));
        } else if let Some(s) = &v.secret {
            w.kv(i, "Type", "Secret (a volume populated by a Secret)");
            w.kv(i, "SecretName", or_none(s.secret_name.as_deref()));
            w.kv(i, "Optional", s.optional.unwrap_or(false));
        } else if let Some(e) = &v.empty_dir {
            w.kv(
                i,
                "Type",
                "EmptyDir (a temporary directory that shares a pod's lifetime)",
            );
            w.kv(i, "Medium", e.medium.as_deref().unwrap_or(""));
            w.kv(
                i,
                "SizeLimit",
                e.size_limit.as_ref().map(|q| q.0.as_str()).unwrap_or(UNSET),
            );
        } else if let Some(h) = &v.host_path {
            w.kv(i, "Type", "HostPath (bare host directory volume)");
            w.kv(i, "Path", &h.path);
            w.kv(i, "HostPathType", h.type_.as_deref().unwrap_or(""));
        } else if let Some(p) = &v.persistent_volume_claim {
            w.kv(
                i,
                "Type",
                "PersistentVolumeClaim (a reference to a PersistentVolumeClaim in the same namespace)",
            );
            w.kv(i, "ClaimName", &p.claim_name);
            w.kv(i, "ReadOnly", p.read_only.unwrap_or(false));
        } else if let Some(p) = &v.projected {
            w.kv(
                i,
                "Type",
                "Projected (a volume that contains injected data from multiple sources)",
            );
            for s in p.sources.as_deref().unwrap_or(&[]) {
                if let Some(secret) = &s.secret {
                    w.kv(i, "SecretName", &secret.name);
                    w.kv(i, "Optional", secret.optional.unwrap_or(false));
                } else if s.downward_api.is_some() {
                    w.kv(i, "DownwardAPI", "true");
                } else if let Some(config_map) = &s.config_map {
                    w.kv(i, "ConfigMapName", &config_map.name);
                    w.kv(i, "Optional", config_map.optional.unwrap_or(false));
                } else if let Some(t) = &s.service_account_token {
                    w.kv(
                        i,
                        "TokenExpirationSeconds",
                        t.expiration_seconds
                            .map(|e| e.to_string())
                            .unwrap_or_else(|| UNSET.into()),
                    );
                }
            }
        } else {
            w.kv(i, "Type", UNKNOWN);
        }
    }
}

/// `key=value:Effect op=Exists for 300s` per toleration.
pub fn toleration_lines(tols: Option<&[Toleration]>) -> Vec<String> {
    let mut tolerations: Vec<_> = tols.unwrap_or(&[]).iter().collect();
    tolerations.sort_by(|a, b| {
        a.key
            .as_deref()
            .unwrap_or("")
            .cmp(b.key.as_deref().unwrap_or(""))
    });
    tolerations
        .into_iter()
        .map(|t| {
            let mut s = t.key.clone().unwrap_or_default();
            if let Some(v) = &t.value {
                s.push('=');
                s.push_str(v);
            }
            if let Some(e) = &t.effect {
                s.push(':');
                s.push_str(e);
            }
            if t.operator.as_deref() == Some("Exists") {
                if s.is_empty() {
                    s.push_str("op=Exists");
                } else {
                    s.push_str(" op=Exists");
                }
            }
            if let Some(secs) = t.toleration_seconds {
                s.push_str(&format!(" for {secs}s"));
            }
            s
        })
        .collect()
}

/// `<topologyKey>:<whenUnsatisfiable> when max skew <n> is exceeded`, plus
/// ` for selector <selector>` when the constraint carries one.
fn write_topology_spread_constraints(
    w: &mut Writer,
    level: usize,
    constraints: Option<&[TopologySpreadConstraint]>,
) {
    let Some(constraints) = constraints.filter(|c| !c.is_empty()) else {
        return;
    };
    // Sorted by topologyKey, mirroring kubectl's printTopologySpreadConstraintsMultilineWithIndent.
    let mut sorted: Vec<&TopologySpreadConstraint> = constraints.iter().collect();
    sorted.sort_by(|a, b| a.topology_key.cmp(&b.topology_key));
    let lines: Vec<String> = sorted
        .into_iter()
        .map(|c| {
            let mut line = format!(
                "{}:{} when max skew {} is exceeded",
                c.topology_key, c.when_unsatisfiable, c.max_skew
            );
            if let Some(selector) = &c.label_selector {
                line.push_str(&format!(" for selector {}", selector_string(selector)));
            }
            line
        })
        .collect();
    write_list(w, level, "Topology Spread Constraints", &lines);
}

/// `Pod Template:` block shared by every workload describer.
pub fn write_pod_template(w: &mut Writer, level: usize, t: &PodTemplateSpec) {
    w.section(level, "Pod Template");
    let i = level + 1;
    let meta = t.metadata.clone().unwrap_or_default();
    write_list(w, i, "Labels", &map_lines(meta.labels.as_ref()));
    if meta.annotations.as_ref().is_some_and(|a| !a.is_empty()) {
        write_list(
            w,
            i,
            "Annotations",
            &annotation_lines(meta.annotations.as_ref()),
        );
    }
    let Some(spec) = t.spec.as_ref() else {
        return;
    };
    if let Some(sa) = spec
        .service_account_name
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        w.kv(i, "Service Account", sa);
    }
    if let Some(ic) = spec.init_containers.as_deref().filter(|v| !v.is_empty()) {
        write_containers(
            w,
            i,
            TEMPLATE_NAME_PREFIX,
            "Init Containers",
            ic,
            None,
            None,
        );
    }
    write_containers(
        w,
        i,
        TEMPLATE_NAME_PREFIX,
        "Containers",
        &spec.containers,
        None,
        None,
    );
    write_volumes(w, i, TEMPLATE_NAME_PREFIX, spec.volumes.as_deref());
    write_topology_spread_constraints(w, i, spec.topology_spread_constraints.as_deref());
    if let Some(pc) = spec.priority_class_name.as_deref() {
        w.kv(i, "Priority Class Name", pc);
    }
    write_list(
        w,
        i,
        "Node-Selectors",
        &map_lines(spec.node_selector.as_ref()),
    );
    write_list(
        w,
        i,
        "Tolerations",
        &toleration_lines(spec.tolerations.as_deref()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::describe::util::test_support::normalize;
    use k8s_openapi::api::core::v1::{ExecAction, HTTPGetAction};
    use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
    use serde_json::json;

    const NOW_MS: i64 = 1_783_080_000_000; // 2026-07-03T12:00:00Z

    fn container(value: serde_json::Value) -> Container {
        serde_json::from_value(value).unwrap()
    }

    fn pod(value: serde_json::Value) -> Pod {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn probe_strings_match_kubectl() {
        let http = Probe {
            http_get: Some(HTTPGetAction {
                path: Some("/healthz".into()),
                port: IntOrString::Int(80),
                ..Default::default()
            }),
            period_seconds: Some(5),
            ..Default::default()
        };
        assert_eq!(
            probe_string(&http),
            "http-get http://:80/healthz delay=0s timeout=1s period=5s #success=1 #failure=3"
        );
        let exec = Probe {
            exec: Some(ExecAction {
                command: Some(vec!["cat".into(), "/tmp/ok".into()]),
            }),
            initial_delay_seconds: Some(10),
            ..Default::default()
        };
        assert_eq!(
            probe_string(&exec),
            "exec [cat /tmp/ok] delay=10s timeout=1s period=10s #success=1 #failure=3"
        );
    }

    #[test]
    fn tolerations_format_like_kubectl() {
        let t = Toleration {
            key: Some("node.kubernetes.io/not-ready".into()),
            operator: Some("Exists".into()),
            effect: Some("NoExecute".into()),
            toleration_seconds: Some(300),
            value: None,
        };
        assert_eq!(
            toleration_lines(Some(&[t]))[0],
            "node.kubernetes.io/not-ready:NoExecute op=Exists for 300s"
        );
        let kv = Toleration {
            key: Some("k".into()),
            value: Some("v".into()),
            effect: Some("NoSchedule".into()),
            operator: Some("Equal".into()),
            toleration_seconds: None,
        };
        assert_eq!(toleration_lines(Some(&[kv]))[0], "k=v:NoSchedule");
        assert!(toleration_lines(None).is_empty());
    }

    #[test]
    fn env_value_from_resolves_with_pod_context() {
        let pod = pod(json!({
            "metadata": {"name": "web-1"},
            "spec": {"containers": [{
                "name": "web",
                "image": "nginx",
                "resources": {"requests": {"cpu": "250m"}},
                "env": [
                    {"name": "POD_NAME", "valueFrom": {"fieldRef": {
                        "apiVersion": "v1", "fieldPath": "metadata.name"
                    }}},
                    {"name": "CPU_MILLIS", "valueFrom": {"resourceFieldRef": {
                        "resource": "requests.cpu", "divisor": "1m"
                    }}}
                ]
            }]}
        }));
        let container = &pod.spec.as_ref().unwrap().containers[0];
        let mut w = Writer::new();
        write_container(&mut w, 0, "", container, None, Some(&pod));
        let output = w.finish();
        let pod_name = output
            .lines()
            .find(|line| line.contains("POD_NAME:"))
            .unwrap();
        assert_eq!(
            pod_name.split_whitespace().collect::<Vec<_>>().join(" "),
            "POD_NAME: web-1 (v1:metadata.name)"
        );
        let cpu = output
            .lines()
            .find(|line| line.contains("CPU_MILLIS:"))
            .unwrap();
        assert_eq!(
            cpu.split_whitespace().collect::<Vec<_>>().join(" "),
            "CPU_MILLIS: 250 (requests.cpu)"
        );

        let mut w = Writer::new();
        write_container(&mut w, 0, "", container, None, None);
        let output = w.finish();
        let field_line = output
            .lines()
            .find(|line| line.contains("POD_NAME:"))
            .unwrap();
        assert!(field_line.ends_with(" (v1:metadata.name)"));
        assert!(!field_line.contains("web-1"));
    }

    #[test]
    fn multiline_command_args_and_env_values_use_continuation_lines() {
        let container = container(json!({
            "name": "web",
            "image": "nginx",
            "command": ["first\nsecond"],
            "args": ["third\nfourth"],
            "env": [{"name": "MULTI", "value": "alpha\nbeta"}]
        }));
        let mut w = Writer::new();
        write_container(&mut w, 0, "", &container, None, None);
        let output = w.finish();
        assert!(output.contains("  Command:\n    first\n    second\n"));
        assert!(output.contains("  Args:\n    third\n    fourth\n"));
        assert!(output.contains("    MULTI:  alpha\n            beta\n"));
    }

    #[test]
    fn sparse_pending_pod_prints_empty_ids_ips_and_named_ports() {
        let pod = pod(json!({
            "metadata": {"name": "pending", "namespace": "default"},
            "spec": {"containers": [{
                "name": "web", "image": "nginx",
                "ports": [{"name": "http", "containerPort": 80}]
            }]},
            "status": {
                "phase": "Pending",
                "containerStatuses": [{
                    "name": "web", "image": "nginx", "imageID": "",
                    "ready": false, "restartCount": 0
                }]
            }
        }));
        let mut w = Writer::new();
        write(&mut w, &pod, NOW_MS);
        let output = w.finish();
        assert!(output.lines().any(|line| {
            line.split_whitespace().collect::<Vec<_>>().join(" ") == "IPs: <none>"
        }));
        assert!(output
            .lines()
            .any(|line| line.trim_end() == "    Container ID:"));
        assert!(output.lines().any(|line| {
            line.split_whitespace().collect::<Vec<_>>().join(" ") == "Port: 80/TCP (http)"
        }));
        assert!(output.lines().any(|line| {
            line.split_whitespace().collect::<Vec<_>>().join(" ") == "Host Port: 0/TCP (http)"
        }));
    }

    #[test]
    fn projected_volumes_print_every_supported_source() {
        let volume: Volume = serde_json::from_value(json!({
            "name": "projected",
            "projected": {"sources": [
                {"serviceAccountToken": {"path": "token", "expirationSeconds": 3600}},
                {"configMap": {"name": "cfg", "optional": true}},
                {"downwardAPI": {}},
                {"secret": {"name": "credentials", "optional": false}}
            ]}
        }))
        .unwrap();
        let mut w = Writer::new();
        write_volumes(&mut w, 0, "", Some(&[volume]));
        let output = w.finish();
        let token = output.find("TokenExpirationSeconds:").unwrap();
        let config_map = output.find("ConfigMapName:").unwrap();
        let downward = output.find("DownwardAPI:").unwrap();
        let secret = output.find("SecretName:").unwrap();
        assert!(token < config_map && config_map < downward && downward < secret);
        assert!(output.contains("ConfigMapName:") && output.contains("cfg"));
        assert!(output.contains("SecretName:") && output.contains("credentials"));
        assert_eq!(output.matches("Optional:").count(), 2);
        assert!(!output.contains("ConfigMapOptional:"));
    }

    #[test]
    fn deleted_pod_reports_termination_and_grace_period() {
        let mut pod = pod(json!({
            "metadata": {
                "name": "web-1",
                "namespace": "default",
                "deletionTimestamp": "2026-07-03T11:50:00Z",
                "deletionGracePeriodSeconds": 30
            },
            "spec": {"containers": []},
            "status": {"phase": "Running"}
        }));
        let mut w = Writer::new();
        write(&mut w, &pod, NOW_MS);
        let output = normalize(&w.finish());
        assert!(output.contains("Status:  Terminating (lasts 10m)\n"));
        assert!(output.contains("Termination Grace Period:  30s\n"));

        pod.status.as_mut().unwrap().phase = Some("Succeeded".into());
        let mut w = Writer::new();
        write(&mut w, &pod, NOW_MS);
        let output = normalize(&w.finish());
        assert!(output.contains("Status:  Succeeded\n"));
        assert!(!output.contains("Termination Grace Period:"));
    }

    #[test]
    fn topology_spread_constraints_are_sorted_and_omitted_when_empty() {
        let constraints: Vec<TopologySpreadConstraint> = serde_json::from_value(json!([
            {"maxSkew": 2, "topologyKey": "topology.kubernetes.io/zone", "whenUnsatisfiable": "DoNotSchedule"},
            {
                "maxSkew": 1,
                "topologyKey": "kubernetes.io/hostname",
                "whenUnsatisfiable": "ScheduleAnyway",
                "labelSelector": {"matchLabels": {"app": "web"}}
            }
        ]))
        .unwrap();
        let mut w = Writer::new();
        write_topology_spread_constraints(&mut w, 0, Some(&constraints));
        assert_eq!(
            normalize(&w.finish()),
            concat!(
                "Topology Spread Constraints:  kubernetes.io/hostname:ScheduleAnyway when max skew 1 is exceeded for selector app=web\n",
                "                              topology.kubernetes.io/zone:DoNotSchedule when max skew 2 is exceeded\n",
            )
        );

        let mut w = Writer::new();
        write_topology_spread_constraints(&mut w, 0, Some(&[]));
        write_topology_spread_constraints(&mut w, 0, None);
        assert_eq!(w.finish(), "");
    }

    #[test]
    fn ephemeral_containers_use_the_container_printer_and_their_statuses() {
        let pod = pod(json!({
            "metadata": {"name": "web-1", "namespace": "default"},
            "spec": {
                "containers": [{"name": "web", "image": "nginx"}],
                "ephemeralContainers": [{
                    "name": "debugger",
                    "image": "busybox:1.36",
                    "command": ["sh"],
                    "targetContainerName": "web"
                }]
            },
            "status": {
                "phase": "Running",
                "ephemeralContainerStatuses": [{
                    "name": "debugger", "image": "busybox:1.36", "imageID": "sha256:def",
                    "ready": false, "restartCount": 1, "state": {"waiting": {"reason": "Starting"}}
                }]
            }
        }));
        let mut w = Writer::new();
        write(&mut w, &pod, NOW_MS);
        let output = normalize(&w.finish());

        let section = output.find("Ephemeral Containers:\n").unwrap();
        assert!(section > output.find("Containers:\n").unwrap());
        assert!(section < output.find("Volumes:").unwrap());
        assert!(output.contains("  debugger:\n"));
        assert!(output.contains("    Image:  busybox:1.36\n"));
        assert!(output.contains("    State:  Waiting\n"));
        assert!(output.contains("      Reason:  Starting\n"));
        assert!(output.contains("    Restart Count:  1\n"));
    }

    #[test]
    fn pod_template_indents_names_one_space_deeper_than_the_header() {
        let template: PodTemplateSpec = serde_json::from_value(json!({
            "metadata": {"labels": {"app": "web"}},
            "spec": {
                "containers": [{"name": "web", "image": "nginx", "resources": {"limits": {"cpu": "1"}}}],
                "volumes": [{"name": "cfg", "configMap": {"name": "cfg"}}]
            }
        }))
        .unwrap();
        let mut w = Writer::new();
        write_pod_template(&mut w, 0, &template);
        let output = normalize(&w.finish());

        assert!(output.contains("  Containers:\n   web:\n    Image:  nginx\n"));
        assert!(output.contains("    Limits:\n      cpu:  1\n"));
        assert!(output.contains("  Volumes:\n   cfg:\n    Type:  ConfigMap"));
    }

    #[test]
    fn mounts_tolerations_and_prefixed_env_from_match_kubectl_ordering() {
        let container = container(json!({
            "name": "web",
            "image": "nginx",
            "envFrom": [{"prefix": "APP_", "configMapRef": {"name": "cfg", "optional": true}}],
            "volumeMounts": [
                {"name": "z", "mountPath": "/z"},
                {"name": "a", "mountPath": "/a"}
            ]
        }));
        let mut w = Writer::new();
        write_container(&mut w, 0, "", &container, None, None);
        let output = w.finish();
        assert!(output.find("/a from a").unwrap() < output.find("/z from z").unwrap());
        let env_from = output
            .lines()
            .find(|line| line.contains("ConfigMap with prefix"))
            .unwrap();
        assert_eq!(
            env_from.split_whitespace().collect::<Vec<_>>().join(" "),
            "cfg ConfigMap with prefix 'APP_' Optional: true"
        );

        let tolerations: Vec<Toleration> = serde_json::from_value(json!([
            {"key": "z", "operator": "Exists"},
            {"operator": "Exists"},
            {"key": "a", "effect": "NoSchedule", "operator": "Exists"}
        ]))
        .unwrap();
        assert_eq!(
            toleration_lines(Some(&tolerations)),
            ["op=Exists", "a:NoSchedule op=Exists", "z op=Exists"]
        );
    }
}
