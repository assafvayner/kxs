import { describe, expect, it } from "vitest";
import { resolveKind, matchRow, fuzzyKinds } from "./command";
import type { ResourceKind } from "./api";

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
