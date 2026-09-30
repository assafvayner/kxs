import type { ContainerInfo } from "./api";

export interface PickOption {
  label: string;
  hint?: string;
}

export interface PortChoice {
  port: number;
  container: string;
  label: string;
}

/** Containers a shell can attach to: init containers have already terminated by
 * the time a pod runs, so they are never exec targets. */
export function execContainers(infos: ContainerInfo[]): ContainerInfo[] {
  return infos.filter((c) => !c.initContainer || c.sidecar);
}

export function containerOptions(infos: ContainerInfo[]): PickOption[] {
  return infos.map((c) => ({
    label: c.name,
    hint: c.ready ? c.image : `${c.image} · not ready`,
  }));
}

/** Declared containerPorts of a pod's non-init containers. Forwarding targets
 * the pod's network namespace, so ports are deduplicated across containers. */
export function portChoices(infos: ContainerInfo[]): PortChoice[] {
  const out: PortChoice[] = [];
  const seen = new Set<number>();
  for (const c of execContainers(infos)) {
    for (const p of c.ports) {
      if (seen.has(p.containerPort)) continue;
      seen.add(p.containerPort);
      out.push({
        port: p.containerPort,
        container: c.name,
        label: p.name
          ? `${c.name} ${p.containerPort} (${p.name})`
          : `${c.name} ${p.containerPort}`,
      });
    }
  }
  return out;
}
