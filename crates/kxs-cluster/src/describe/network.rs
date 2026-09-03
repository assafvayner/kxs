use super::header::{annotation_lines, write_labels_annotations};
use super::util::{int_or_string, map_lines, or_none, write_list, NONE, UNSET};
use super::writer::Writer;
use k8s_openapi::api::core::v1::{EndpointSubset, Endpoints, Service, ServicePort};
use k8s_openapi::api::networking::v1::{Ingress, IngressBackend};
use std::collections::BTreeSet;

/// kubectl's placeholder for an Ingress without an explicit default backend.
const DEFAULT_BACKEND: &str = "<default>";

/// kubectl's `formatEndpoints`: bare IPs when a subset has no ports, otherwise
/// `ip:port` for ports matching the service port name. At most 3 entries are
/// shown across all subsets, followed by `+ N more...`; `<none>` when empty.
pub fn format_endpoints(eps: Option<&Endpoints>, port_name: Option<&str>) -> String {
    let Some(eps) = eps else {
        return NONE.into();
    };
    let mut out: Vec<String> = Vec::new();
    for subset in eps.subsets.as_deref().unwrap_or(&[]) {
        let addresses = subset.addresses.as_deref().unwrap_or(&[]);
        let ports = subset.ports.as_deref().unwrap_or(&[]);
        if ports.is_empty() {
            out.extend(addresses.iter().map(|addr| addr.ip.clone()));
        } else {
            let service_port_name = port_name.unwrap_or_default();
            for port in ports {
                if port.name.as_deref().unwrap_or_default() != service_port_name {
                    continue;
                }
                for addr in addresses {
                    let host = if addr.ip.contains(':') {
                        format!("[{}]", addr.ip)
                    } else {
                        addr.ip.clone()
                    };
                    out.push(format!("{host}:{}", port.port));
                }
            }
        }
    }
    if out.is_empty() {
        return NONE.into();
    }
    if out.len() > 3 {
        let more = out.len() - 3;
        out.truncate(3);
        return format!("{} + {more} more...", out.join(","));
    }
    out.join(",")
}

fn write_port(w: &mut Writer, svc_port: &ServicePort, eps: Option<&Endpoints>) {
    let proto = svc_port.protocol.as_deref().unwrap_or("TCP");
    let name = svc_port
        .name
        .as_deref()
        .filter(|n| !n.is_empty())
        .unwrap_or(UNSET);
    let port = format!("{}/{proto}", svc_port.port);
    w.cells(0, &["Port:", name, &port]);
    let target = svc_port
        .target_port
        .as_ref()
        .map(int_or_string)
        .unwrap_or_else(|| svc_port.port.to_string());
    w.kv(0, "TargetPort", format!("{target}/{proto}"));
    if let Some(np) = svc_port.node_port {
        let np = format!("{np}/{proto}");
        w.cells(0, &["NodePort:", name, &np]);
    }
    w.kv(
        0,
        "Endpoints",
        format_endpoints(eps, svc_port.name.as_deref()),
    );
}

pub fn write_service(w: &mut Writer, svc: &Service, eps: Option<&Endpoints>) {
    let meta = &svc.metadata;
    let spec = svc.spec.as_ref();
    w.kv(0, "Name", or_none(meta.name.as_deref()));
    w.kv(0, "Namespace", or_none(meta.namespace.as_deref()));
    write_labels_annotations(w, meta);
    let selector = map_lines(spec.and_then(|s| s.selector.as_ref()));
    w.kv(
        0,
        "Selector",
        if selector.is_empty() {
            NONE.to_string()
        } else {
            selector.join(",")
        },
    );
    w.kv(
        0,
        "Type",
        spec.and_then(|s| s.type_.as_deref()).unwrap_or("ClusterIP"),
    );
    if let Some(p) = spec.and_then(|s| s.ip_family_policy.as_deref()) {
        w.kv(0, "IP Family Policy", p);
    }
    if let Some(f) = spec
        .and_then(|s| s.ip_families.as_ref())
        .filter(|f| !f.is_empty())
    {
        w.kv(0, "IP Families", f.join(","));
    } else {
        w.kv(0, "IP Families", NONE);
    }
    w.kv(
        0,
        "IP",
        spec.and_then(|s| s.cluster_ip.as_deref())
            .unwrap_or_default(),
    );
    let ips = spec
        .and_then(|s| s.cluster_ips.as_ref())
        .map(|v| v.join(","))
        .unwrap_or_default();
    w.kv(0, "IPs", or_none(Some(&ips)));
    if let Some(ext) = spec
        .and_then(|s| s.external_ips.as_ref())
        .filter(|v| !v.is_empty())
    {
        w.kv(0, "External IPs", ext.join(","));
    }
    if let Some(lb) = spec.and_then(|s| s.load_balancer_ip.as_deref()) {
        w.kv(0, "Desired LoadBalancer IP", lb);
    }
    if let Some(name) = spec
        .and_then(|s| s.external_name.as_deref())
        .filter(|name| !name.is_empty())
    {
        w.kv(0, "External Name", name);
    }
    let ingress: Vec<String> = svc
        .status
        .as_ref()
        .and_then(|s| s.load_balancer.as_ref())
        .and_then(|lb| lb.ingress.as_ref())
        .map(|v| {
            v.iter()
                .filter_map(|i| {
                    if let Some(ip) = i.ip.as_deref().filter(|ip| !ip.is_empty()) {
                        Some(match i.ip_mode.as_deref() {
                            Some(mode) => format!("{ip} ({mode})"),
                            None => ip.to_string(),
                        })
                    } else {
                        i.hostname.clone().filter(|hostname| !hostname.is_empty())
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    if !ingress.is_empty() {
        w.kv(0, "LoadBalancer Ingress", ingress.join(", "));
    }
    for p in spec.and_then(|s| s.ports.as_deref()).unwrap_or(&[]) {
        write_port(w, p, eps);
    }
    w.kv(
        0,
        "Session Affinity",
        spec.and_then(|s| s.session_affinity.as_deref())
            .unwrap_or("None"),
    );
    if let Some(p) = spec.and_then(|s| s.external_traffic_policy.as_deref()) {
        w.kv(0, "External Traffic Policy", p);
    }
    if let Some(p) = spec.and_then(|s| s.internal_traffic_policy.as_deref()) {
        w.kv(0, "Internal Traffic Policy", p);
    }
    if let Some(port) = spec
        .and_then(|s| s.health_check_node_port)
        .filter(|port| *port != 0)
    {
        w.kv(0, "HealthCheck NodePort", port);
    }
    if let Some(ranges) = spec
        .and_then(|s| s.load_balancer_source_ranges.as_deref())
        .filter(|ranges| !ranges.is_empty())
    {
        w.kv(0, "LoadBalancer Source Ranges", ranges.join(","));
    }
}

fn addresses(list: Option<&[k8s_openapi::api::core::v1::EndpointAddress]>) -> String {
    let ips: Vec<&str> = list.unwrap_or(&[]).iter().map(|a| a.ip.as_str()).collect();
    if ips.is_empty() {
        NONE.to_string()
    } else {
        ips.join(",")
    }
}

fn write_subset(w: &mut Writer, s: &EndpointSubset) {
    w.kv(1, "Addresses", addresses(s.addresses.as_deref()));
    w.kv(
        1,
        "NotReadyAddresses",
        addresses(s.not_ready_addresses.as_deref()),
    );
    let ports = s.ports.as_deref().unwrap_or(&[]);
    if ports.is_empty() {
        w.kv(1, "Ports", NONE);
        return;
    }
    w.section(1, "Ports");
    w.cells(2, &["Name", "Port", "Protocol"]);
    w.cells(2, &["----", "----", "--------"]);
    for p in ports {
        let port = p.port.to_string();
        w.cells(
            2,
            &[
                p.name.as_deref().unwrap_or(UNSET),
                &port,
                p.protocol.as_deref().unwrap_or("TCP"),
            ],
        );
    }
}

pub fn write_endpoints(w: &mut Writer, eps: &Endpoints) {
    let meta = &eps.metadata;
    w.kv(0, "Name", or_none(meta.name.as_deref()));
    w.kv(0, "Namespace", or_none(meta.namespace.as_deref()));
    write_labels_annotations(w, meta);
    let subsets = eps.subsets.as_deref().unwrap_or(&[]);
    if subsets.is_empty() {
        w.kv(0, "Subsets", NONE);
        return;
    }
    w.section(0, "Subsets");
    for s in subsets {
        write_subset(w, s);
        w.text(0, "");
    }
}

fn backend_string(b: &IngressBackend) -> String {
    if let Some(svc) = &b.service {
        let port = svc
            .port
            .as_ref()
            .and_then(|p| {
                p.number
                    .filter(|number| *number != 0)
                    .map(|number| number.to_string())
                    .or_else(|| p.name.clone())
            })
            .unwrap_or_default();
        return format!("{}:{port}", svc.name);
    }
    if let Some(r) = &b.resource {
        return format!(
            "APIGroup: {}, Kind: {}, Name: {}",
            r.api_group.as_deref().unwrap_or(NONE),
            r.kind,
            r.name
        );
    }
    NONE.into()
}

pub fn write_ingress(w: &mut Writer, ing: &Ingress) {
    let meta = &ing.metadata;
    let spec = ing.spec.as_ref();
    w.kv(0, "Name", or_none(meta.name.as_deref()));
    write_list(w, 0, "Labels", &map_lines(meta.labels.as_ref()));
    w.kv(0, "Namespace", or_none(meta.namespace.as_deref()));
    let address: BTreeSet<&str> = ing
        .status
        .as_ref()
        .and_then(|s| s.load_balancer.as_ref())
        .and_then(|lb| lb.ingress.as_ref())
        .map(|v| {
            v.iter()
                .filter_map(|i| {
                    i.ip.as_deref().filter(|ip| !ip.is_empty()).or_else(|| {
                        i.hostname
                            .as_deref()
                            .filter(|hostname| !hostname.is_empty())
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    w.kv(
        0,
        "Address",
        address.into_iter().collect::<Vec<_>>().join(","),
    );
    w.kv(
        0,
        "Ingress Class",
        or_none(spec.and_then(|s| s.ingress_class_name.as_deref())),
    );
    let default_backend = spec
        .and_then(|s| s.default_backend.as_ref())
        .map(backend_string);
    w.kv(
        0,
        "Default backend",
        default_backend.as_deref().unwrap_or(DEFAULT_BACKEND),
    );
    if let Some(tls) = spec
        .and_then(|s| s.tls.as_deref())
        .filter(|t| !t.is_empty())
    {
        w.section(0, "TLS");
        for t in tls {
            let hosts = t.hosts.as_deref().unwrap_or(&[]).join(",");
            let line = match t.secret_name.as_deref().filter(|name| !name.is_empty()) {
                Some(name) => format!("{name} terminates {hosts}"),
                None => format!("SNI routes {hosts}"),
            };
            w.text(1, &line);
        }
    }
    w.section(0, "Rules");
    w.cells(1, &["Host", "Path", "Backends"]);
    w.cells(1, &["----", "----", "--------"]);
    let rules = spec.and_then(|s| s.rules.as_deref()).unwrap_or(&[]);
    let mut rendered_rule = false;
    for r in rules {
        let Some(http) = r.http.as_ref() else {
            continue;
        };
        rendered_rule = true;
        let host = r.host.as_deref().filter(|host| !host.is_empty());
        w.cells(1, &[host.unwrap_or("*"), ""]);
        for p in &http.paths {
            // kubectl writes the path row as `\t%s \t%s`, so the Host column
            // holds nothing but LEVEL_2's indent and the path lands in Path.
            let path = format!("{} ", p.path.as_deref().unwrap_or_default());
            let backend = backend_string(&p.backend);
            w.cells(0, &["    ", &path, &backend]);
        }
    }
    if !rendered_rule {
        w.cells(
            1,
            &[
                "*",
                "*",
                default_backend.as_deref().unwrap_or(DEFAULT_BACKEND),
            ],
        );
    }
    write_list(
        w,
        0,
        "Annotations",
        &annotation_lines(meta.annotations.as_ref()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{EndpointAddress, EndpointPort};
    use serde_json::json;

    fn endpoint_port(name: Option<&str>, port: i32) -> EndpointPort {
        EndpointPort {
            name: name.map(str::to_string),
            port,
            ..Default::default()
        }
    }

    fn subset(addresses: &[&str], ports: Option<Vec<EndpointPort>>) -> EndpointSubset {
        EndpointSubset {
            addresses: Some(
                addresses
                    .iter()
                    .map(|ip| EndpointAddress {
                        ip: (*ip).to_string(),
                        ..Default::default()
                    })
                    .collect(),
            ),
            ports,
            ..Default::default()
        }
    }

    fn endpoints(subsets: Vec<EndpointSubset>) -> Endpoints {
        Endpoints {
            subsets: Some(subsets),
            ..Default::default()
        }
    }

    fn render_service(value: serde_json::Value) -> String {
        let service: Service = serde_json::from_value(value).unwrap();
        let mut writer = Writer::new();
        write_service(&mut writer, &service, None);
        writer.finish()
    }

    fn render_ingress(value: serde_json::Value) -> String {
        let ingress: Ingress = serde_json::from_value(value).unwrap();
        let mut writer = Writer::new();
        write_ingress(&mut writer, &ingress);
        writer.finish()
    }

    fn field<'a>(output: &'a str, key: &str) -> &'a str {
        let prefix = format!("{key}:");
        output
            .lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .map(str::trim)
            .unwrap_or_else(|| panic!("missing {key:?} in:\n{output}"))
    }

    fn assert_field_order(output: &str, fields: &[&str]) {
        let mut previous = None;
        for field in fields {
            let prefix = format!("{field}:");
            let index = output
                .lines()
                .position(|line| line.starts_with(&prefix))
                .unwrap_or_else(|| panic!("missing {field:?} in:\n{output}"));
            if let Some(previous) = previous {
                assert!(index > previous, "{field:?} is out of order in:\n{output}");
            }
            previous = Some(index);
        }
    }

    #[test]
    fn unnamed_service_port_only_matches_unnamed_endpoint_port() {
        let eps = endpoints(vec![subset(
            &["10.0.0.1"],
            Some(vec![
                endpoint_port(Some("http"), 80),
                endpoint_port(None, 81),
            ]),
        )]);

        assert_eq!(format_endpoints(Some(&eps), None), "10.0.0.1:81");
        assert_eq!(format_endpoints(Some(&eps), Some("")), "10.0.0.1:81");
    }

    #[test]
    fn subsets_without_ports_emit_bare_ips() {
        let eps = endpoints(vec![
            subset(&["10.0.0.1"], None),
            subset(&["10.0.0.2"], Some(vec![])),
        ]);

        assert_eq!(
            format_endpoints(Some(&eps), Some("http")),
            "10.0.0.1,10.0.0.2"
        );
    }

    #[test]
    fn ipv6_endpoints_use_bracketed_host_port() {
        let eps = endpoints(vec![subset(
            &["2001:db8::10"],
            Some(vec![endpoint_port(Some("https"), 443)]),
        )]);

        assert_eq!(
            format_endpoints(Some(&eps), Some("https")),
            "[2001:db8::10]:443"
        );
    }

    #[test]
    fn endpoint_cap_and_remaining_count_span_subsets_in_order() {
        let eps = endpoints(vec![
            subset(
                &["10.0.0.1", "10.0.0.2"],
                Some(vec![endpoint_port(Some("http"), 80)]),
            ),
            subset(
                &["10.0.0.3", "10.0.0.4"],
                Some(vec![
                    endpoint_port(Some("grpc"), 9000),
                    endpoint_port(Some("http"), 8080),
                ]),
            ),
            subset(&["10.0.0.5"], None),
        ]);

        assert_eq!(
            format_endpoints(Some(&eps), Some("http")),
            "10.0.0.1:80,10.0.0.2:80,10.0.0.3:8080 + 2 more..."
        );
    }

    #[test]
    fn missing_or_unmatched_endpoints_render_none() {
        let eps = endpoints(vec![subset(
            &["10.0.0.1"],
            Some(vec![endpoint_port(Some("http"), 80)]),
        )]);

        assert_eq!(format_endpoints(Some(&eps), Some("grpc")), NONE);
        assert_eq!(format_endpoints(None, Some("http")), NONE);
    }

    #[test]
    fn external_name_service_prints_empty_ip_in_kubectl_order() {
        for spec in [
            json!({
                "type": "ExternalName",
                "externalName": "external.example",
                "sessionAffinity": "None"
            }),
            json!({
                "type": "ExternalName",
                "clusterIP": "",
                "externalName": "external.example",
                "sessionAffinity": "None"
            }),
        ] {
            let output = render_service(json!({
                "metadata": {"name": "external", "namespace": "default"},
                "spec": spec
            }));

            assert_eq!(field(&output, "IP Families"), NONE);
            assert_eq!(field(&output, "IP"), "");
            assert_eq!(field(&output, "External Name"), "external.example");
            assert_field_order(
                &output,
                &[
                    "Name",
                    "Namespace",
                    "Labels",
                    "Annotations",
                    "Selector",
                    "Type",
                    "IP Families",
                    "IP",
                    "IPs",
                    "External Name",
                    "Session Affinity",
                ],
            );
        }
    }

    #[test]
    fn load_balancer_service_prints_network_details_in_kubectl_order() {
        let output = render_service(json!({
            "metadata": {"name": "load-balancer", "namespace": "default"},
            "spec": {
                "type": "LoadBalancer",
                "clusterIP": "10.96.0.20",
                "clusterIPs": ["10.96.0.20"],
                "ipFamilies": ["IPv4"],
                "loadBalancerIP": "192.0.2.20",
                "ports": [{
                    "name": "https",
                    "port": 443,
                    "targetPort": 8443,
                    "nodePort": 30443,
                    "protocol": "TCP"
                }],
                "sessionAffinity": "None",
                "externalTrafficPolicy": "Local",
                "internalTrafficPolicy": "Cluster",
                "healthCheckNodePort": 32000,
                "loadBalancerSourceRanges": ["192.0.2.0/24", "2001:db8::/64"]
            },
            "status": {"loadBalancer": {"ingress": [
                {"ip": "203.0.113.10", "ipMode": "Proxy"},
                {"ip": "203.0.113.11", "ipMode": "VIP"},
                {"ip": "", "hostname": "lb.example"}
            ]}}
        }));

        assert_eq!(field(&output, "HealthCheck NodePort"), "32000");
        assert_eq!(
            field(&output, "LoadBalancer Source Ranges"),
            "192.0.2.0/24,2001:db8::/64"
        );
        assert_eq!(
            field(&output, "LoadBalancer Ingress"),
            "203.0.113.10 (Proxy), 203.0.113.11 (VIP), lb.example"
        );
        assert_field_order(
            &output,
            &[
                "Name",
                "Namespace",
                "Labels",
                "Annotations",
                "Selector",
                "Type",
                "IP Families",
                "IP",
                "IPs",
                "Desired LoadBalancer IP",
                "LoadBalancer Ingress",
                "Session Affinity",
                "External Traffic Policy",
                "Internal Traffic Policy",
                "HealthCheck NodePort",
                "LoadBalancer Source Ranges",
            ],
        );
    }

    #[test]
    fn ingress_skips_non_http_rules_and_falls_back_to_default_row() {
        let output = render_ingress(json!({
            "metadata": {"name": "web", "namespace": "default"},
            "spec": {
                "defaultBackend": {
                    "service": {"name": "fallback", "port": {"number": 80}}
                },
                "rules": [{"host": "ignored.example"}]
            }
        }));

        assert!(!output.contains("ignored.example"));
        assert!(output.lines().any(|line| {
            line.split_whitespace().collect::<Vec<_>>() == ["*", "*", "fallback:80"]
        }));
    }

    #[test]
    fn ingress_preserves_empty_and_absent_paths() {
        let output = render_ingress(json!({
            "metadata": {"name": "web", "namespace": "default"},
            "spec": {"rules": [{"host": "example.com", "http": {"paths": [
                {"path": "", "pathType": "Prefix", "backend": {
                    "service": {"name": "empty", "port": {"number": 80}}
                }},
                {"pathType": "Prefix", "backend": {
                    "service": {"name": "absent", "port": {"number": 80}}
                }}
            ]}}]}
        }));

        let empty_path = output
            .lines()
            .find(|line| line.contains("empty:80"))
            .unwrap();
        let absent_path = output
            .lines()
            .find(|line| line.contains("absent:80"))
            .unwrap();
        assert_eq!(empty_path.trim(), "empty:80");
        assert_eq!(absent_path.trim(), "absent:80");
    }

    #[test]
    fn ingress_addresses_prefer_ip_then_deduplicate_and_sort() {
        let output = render_ingress(json!({
            "metadata": {"name": "web", "namespace": "default"},
            "status": {"loadBalancer": {"ingress": [
                {"hostname": "z.example"},
                {"ip": "10.0.0.2"},
                {"ip": "", "hostname": "a.example"},
                {"ip": "10.0.0.1", "hostname": "ignored.example"},
                {"ip": "10.0.0.2"},
                {"ip": "", "hostname": ""}
            ]}}
        }));

        assert_eq!(
            field(&output, "Address"),
            "10.0.0.1,10.0.0.2,a.example,z.example"
        );
    }

    #[test]
    fn ingress_without_addresses_prints_an_empty_value() {
        let output = render_ingress(json!({
            "metadata": {"name": "web", "namespace": "default"}
        }));

        assert_eq!(field(&output, "Address"), "");
        assert!(!output.lines().any(|line| line == "Address:  <none>"));
    }

    #[test]
    fn ingress_empty_or_absent_tls_secret_uses_sni_routes() {
        let output = render_ingress(json!({
            "metadata": {"name": "web", "namespace": "default"},
            "spec": {"tls": [
                {"hosts": ["absent.example"]},
                {"hosts": ["empty.example"], "secretName": ""}
            ]}
        }));

        assert!(output.contains("SNI routes absent.example"));
        assert!(output.contains("SNI routes empty.example"));
    }

    #[test]
    fn malformed_zero_backend_port_prefers_name() {
        let backend: IngressBackend = serde_json::from_value(json!({
            "service": {"name": "web", "port": {"number": 0, "name": "http"}}
        }))
        .unwrap();

        assert_eq!(backend_string(&backend), "web:http");
    }
}
