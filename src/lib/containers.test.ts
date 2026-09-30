import { describe, expect, it } from "vitest";
import { containerOptions, execContainers, portChoices } from "./containers";
import type { ContainerInfo, ContainerPortInfo } from "./api";

function c(
  name: string,
  opts: Partial<Omit<ContainerInfo, "name">> = {},
): ContainerInfo {
  return {
    name,
    image: `${name}:1`,
    ready: true,
    restarts: 0,
    ports: [],
    initContainer: false,
    sidecar: false,
    ...opts,
  };
}

function port(containerPort: number, name?: string): ContainerPortInfo {
  return { name: name ?? null, containerPort };
}

describe("execContainers", () => {
  it("drops init containers", () => {
    const infos = [c("migrate", { initContainer: true }), c("web"), c("sidecar")];
    expect(execContainers(infos).map((x) => x.name)).toEqual(["web", "sidecar"]);
  });

  it("keeps native sidecars (init containers with restartPolicy Always)", () => {
    const infos = [
      c("migrate", { initContainer: true }),
      c("proxy", { initContainer: true, sidecar: true }),
      c("web"),
    ];
    expect(execContainers(infos).map((x) => x.name)).toEqual(["proxy", "web"]);
  });
});

describe("containerOptions", () => {
  it("labels by name with the image as hint", () => {
    expect(containerOptions([c("web")])).toEqual([{ label: "web", hint: "web:1" }]);
  });

  it("flags containers that are not ready", () => {
    expect(containerOptions([c("web", { ready: false })])).toEqual([
      { label: "web", hint: "web:1 · not ready" },
    ]);
  });
});

describe("portChoices", () => {
  it("uses the port name when present", () => {
    const infos = [c("web", { ports: [port(8080, "http"), port(9090)] })];
    expect(portChoices(infos)).toEqual([
      { port: 8080, container: "web", label: "web 8080 (http)" },
      { port: 9090, container: "web", label: "web 9090" },
    ]);
  });

  it("deduplicates a port declared by two containers", () => {
    const infos = [c("web", { ports: [port(8080)] }), c("sidecar", { ports: [port(8080)] })];
    expect(portChoices(infos).map((p) => p.container)).toEqual(["web"]);
  });

  it("ignores init container ports", () => {
    const infos = [
      c("migrate", { initContainer: true, ports: [port(5432)] }),
      c("web", { ports: [port(80)] }),
    ];
    expect(portChoices(infos).map((p) => p.port)).toEqual([80]);
  });

  it("is empty when nothing declares a port", () => {
    expect(portChoices([c("web")])).toEqual([]);
  });
});
