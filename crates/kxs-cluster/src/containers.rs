//! Container pick lists, ported from `src/lib/containers.ts`.

use crate::pods::ContainerInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickOption {
    pub label: String,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortChoice {
    pub port: u16,
    pub container: String,
    pub label: String,
}

/// Containers a shell can attach to: init containers have already terminated by
/// the time a pod runs, so they are never exec targets.
pub fn exec_containers(infos: &[ContainerInfo]) -> Vec<ContainerInfo> {
    infos
        .iter()
        .filter(|c| !c.init_container)
        .cloned()
        .collect()
}

pub fn container_options(infos: &[ContainerInfo]) -> Vec<PickOption> {
    infos
        .iter()
        .map(|c| PickOption {
            label: c.name.clone(),
            hint: if c.ready {
                Some(c.image.clone())
            } else {
                Some(format!("{} · not ready", c.image))
            },
        })
        .collect()
}

/// Declared containerPorts of a pod's non-init containers. Forwarding targets
/// the pod's network namespace, so ports are deduplicated across containers.
pub fn port_choices(infos: &[ContainerInfo]) -> Vec<PortChoice> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for c in exec_containers(infos) {
        for p in &c.ports {
            if !seen.insert(p.container_port) {
                continue;
            }
            let label = match &p.name {
                Some(name) => format!("{} {} ({})", c.name, p.container_port, name),
                None => format!("{} {}", c.name, p.container_port),
            };
            out.push(PortChoice {
                port: p.container_port,
                container: c.name.clone(),
                label,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pods::ContainerPortInfo;

    fn c(
        name: &str,
        init: bool,
        ready: bool,
        image: &str,
        ports: Vec<(u16, &str)>,
    ) -> ContainerInfo {
        ContainerInfo {
            name: name.into(),
            image: image.into(),
            ready,
            state: if ready { "running" } else { "waiting" }.into(),
            restarts: 0,
            ports: ports
                .into_iter()
                .map(|(port, pname)| ContainerPortInfo {
                    name: if pname.is_empty() {
                        None
                    } else {
                        Some(pname.into())
                    },
                    container_port: port,
                })
                .collect(),
            init_container: init,
        }
    }

    #[test]
    fn exec_drops_init_containers() {
        let infos = vec![
            c("init", true, false, "init:1", vec![]),
            c("web", false, true, "web:1", vec![]),
            c("sidecar", false, true, "sc:1", vec![]),
        ];
        let execed = exec_containers(&infos);
        let names: Vec<&str> = execed.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["web", "sidecar"]);
    }

    #[test]
    fn options_label_by_name_with_image_hint() {
        let infos = vec![c("web", false, true, "web:1", vec![])];
        let opts = container_options(&infos);
        assert_eq!(
            opts,
            vec![PickOption {
                label: "web".into(),
                hint: Some("web:1".into())
            }]
        );
    }

    #[test]
    fn options_flag_not_ready_containers() {
        let infos = vec![c("web", false, false, "web:1", vec![])];
        let opts = container_options(&infos);
        assert_eq!(opts[0].hint.as_deref(), Some("web:1 · not ready"));
    }

    #[test]
    fn port_choices_use_the_port_name_when_present() {
        let infos = vec![c(
            "web",
            false,
            true,
            "web:1",
            vec![(8080, "http"), (9090, "")],
        )];
        let choices = port_choices(&infos);
        assert_eq!(choices[0].label, "web 8080 (http)");
        assert_eq!(choices[1].label, "web 9090");
    }

    #[test]
    fn port_choices_deduplicate_across_containers() {
        let infos = vec![
            c("web", false, true, "web:1", vec![(8080, "http")]),
            c("sidecar", false, true, "sc:1", vec![(8080, "metrics")]),
        ];
        let choices = port_choices(&infos);
        let containers: Vec<&str> = choices.iter().map(|p| p.container.as_str()).collect();
        assert_eq!(containers, vec!["web"]);
    }

    #[test]
    fn port_choices_ignore_init_container_ports() {
        let infos = vec![
            c("init", true, false, "init:1", vec![(9999, "x")]),
            c("web", false, true, "web:1", vec![(80, "")]),
        ];
        let ports: Vec<u16> = port_choices(&infos).iter().map(|p| p.port).collect();
        assert_eq!(ports, vec![80]);
    }

    #[test]
    fn port_choices_empty_when_no_ports() {
        let infos = vec![c("web", false, true, "web:1", vec![])];
        assert!(port_choices(&infos).is_empty());
    }
}
