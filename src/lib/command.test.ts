import { describe, expect, it } from "vitest";
import { resolveKind, matchRow, fuzzyKinds, visibleKinds, splitFilter } from "./command";
import { currentKindLabel } from "./command";
import { searchEnabled } from "./command";
import type { ResourceKind } from "./api";
import type { View } from "./stores/viewstack.svelte";

const kinds: ResourceKind[] = [
  { group: "", version: "v1", kind: "Pod", plural: "pods", namespaced: true, aliases: ["po", "pod", "pods"] },
  { group: "apps", version: "v1", kind: "Deployment", plural: "deployments", namespaced: true, aliases: ["deploy", "deployment", "deployments"] },
  { group: "", version: "v1", kind: "Service", plural: "services", namespaced: true, aliases: ["service", "services", "svc"] },
];

describe("resolveKind", () => {
  it("matches exact alias case-insensitively", () => {
    expect(resolveKind(kinds, "po")?.kind).toBe("Pod");
    expect(resolveKind(kinds, "PODS")?.kind).toBe("Pod");
    expect(resolveKind(kinds, "svc")?.kind).toBe("Service");
    expect(resolveKind(kinds, "deploy")?.kind).toBe("Deployment");
  });
  it("returns null for unknown", () => {
    expect(resolveKind(kinds, "nope")).toBeNull();
  });
});

describe("fuzzyKinds", () => {
  it("ranks exact-alias first, then substring", () => {
    const r = fuzzyKinds(kinds, "dep");
    expect(r[0].kind).toBe("Deployment");
  });
  it("empty query returns all", () => {
    expect(fuzzyKinds(kinds, "").length).toBe(3);
  });
});

describe("visibleKinds", () => {
  const node: ResourceKind = {
    group: "", version: "v1", kind: "Node", plural: "nodes", namespaced: false, aliases: ["no"],
  };
  const all = [...kinds, node];

  it("returns all kinds when not yet probed (null)", () => {
    expect(visibleKinds(all, null)).toEqual(all);
  });

  it("keeps only namespaced kinds present in the set", () => {
    const present = new Set(["/Pod"]);
    const r = visibleKinds(kinds, present);
    expect(r.map((k) => k.kind)).toEqual(["Pod"]);
  });

  it("always keeps cluster-scoped kinds regardless of the set", () => {
    const r = visibleKinds(all, new Set<string>());
    expect(r.map((k) => k.kind)).toEqual(["Node"]);
  });
});

describe("matchRow", () => {
  it("substring match on name (case-insensitive)", () => {
    expect(matchRow("web-1", "WEB")).toBe(true);
    expect(matchRow("api-xyz", "web")).toBe(false);
  });
  it("regex match with -r prefix", () => {
    expect(matchRow("web-1", "-r ^web")).toBe(true);
    expect(matchRow("api-1", "-r ^web")).toBe(false);
  });
  it("invalid regex falls back to no match, not throw", () => {
    expect(matchRow("web", "-r [")).toBe(false);
  });
  it("empty filter matches everything", () => {
    expect(matchRow("anything", "")).toBe(true);
  });
});

describe("splitFilter", () => {
  it("passes a plain filter through as a name filter", () => {
    expect(splitFilter("web")).toEqual({ labels: null, name: "web" });
    expect(splitFilter("")).toEqual({ labels: null, name: "" });
    expect(splitFilter("-r ^web")).toEqual({ labels: null, name: "-r ^web" });
  });

  it("extracts a label selector on its own", () => {
    expect(splitFilter("-l app=demo-web")).toEqual({ labels: "app=demo-web", name: "" });
    expect(splitFilter("  -l app=demo-web,tier!=db  ")).toEqual({
      labels: "app=demo-web,tier!=db",
      name: "",
    });
  });

  it("extracts a selector plus a trailing name filter", () => {
    expect(splitFilter("-l app=demo-web web-1")).toEqual({
      labels: "app=demo-web",
      name: "web-1",
    });
    expect(splitFilter("-l app=demo-web -r ^web")).toEqual({
      labels: "app=demo-web",
      name: "-r ^web",
    });
  });

  it("treats a bare -l with no selector as no filter at all", () => {
    expect(splitFilter("-l")).toEqual({ labels: null, name: "" });
    expect(splitFilter("-l   ")).toEqual({ labels: null, name: "" });
  });

  it("does not treat -label or -l without a space as a selector", () => {
    expect(splitFilter("-labels")).toEqual({ labels: null, name: "-labels" });
  });

  it("keeps the name part usable by matchRow", () => {
    const { name } = splitFilter("-l app=demo-web -r ^web");
    expect(matchRow("web-1", name)).toBe(true);
    expect(matchRow("api-1", name)).toBe(false);
  });
});

describe("currentKindLabel", () => {
  const dep: ResourceKind = {
    group: "apps", version: "v1", kind: "Deployment", plural: "deployments",
    namespaced: true, aliases: ["deploy"],
  };

  it("returns Pods for the base pods stack", () => {
    expect(currentKindLabel([{ kind: "pods" }])).toBe("Pods");
  });

  it("returns the resource kind when a resource view is on top", () => {
    expect(currentKindLabel([{ kind: "pods" }, { kind: "resource", resourceKind: dep }])).toBe(
      "Deployment",
    );
  });

  it("returns the underlying resource kind when a detail view is on top", () => {
    const views: View[] = [
      { kind: "pods" },
      { kind: "resource", resourceKind: dep },
      { kind: "describe", title: "Deployment web", resourceKind: dep, namespace: "default", name: "web", body: "" },
    ];
    expect(currentKindLabel(views)).toBe("Deployment");
  });

  it("falls back to Pods when no pods/resource view is in the stack", () => {
    expect(currentKindLabel([{ kind: "metrics" }])).toBe("Pods");
  });
});

describe("searchEnabled", () => {
  it("is disabled only on the exec terminal", () => {
    expect(searchEnabled({ kind: "exec", namespace: "default", pod: "p", container: null })).toBe(false);
  });
  it("is enabled on pods, resource, describe, yaml, logs, metrics, forwards", () => {
    expect(searchEnabled({ kind: "pods" })).toBe(true);
    expect(
      searchEnabled({
        kind: "resource",
        resourceKind: { group: "apps", version: "v1", kind: "Deployment", plural: "deployments", namespaced: true, aliases: [] },
      }),
    ).toBe(true);
    expect(
      searchEnabled({
        kind: "describe",
        title: "t",
        resourceKind: { group: "", version: "v1", kind: "Pod", plural: "pods", namespaced: true, aliases: [] },
        namespace: "default",
        name: "n",
        body: "",
      }),
    ).toBe(true);
    expect(searchEnabled({ kind: "yaml", title: "t", body: "" })).toBe(true);
    expect(searchEnabled({ kind: "logs", namespace: "default", pods: ["p"], label: "p" })).toBe(true);
    expect(searchEnabled({ kind: "metrics" })).toBe(true);
    expect(searchEnabled({ kind: "forwards" })).toBe(true);
  });
});
