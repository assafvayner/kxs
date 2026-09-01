import { describe, expect, it } from "vitest";
import { menuItemsFor } from "./contextMenu";
import type { ResourceKind } from "./api";

function kind(k: string, group = ""): ResourceKind {
  return { group, version: "v1", kind: k, plural: `${k.toLowerCase()}s`, namespaced: true, aliases: [] };
}

function ids(k: ResourceKind): string[][] {
  return menuItemsFor(k).map((g) => g.map((i) => i.id));
}

describe("menuItemsFor", () => {
  it("gives every kind inspect actions and delete", () => {
    expect(ids(kind("Ingress", "networking.k8s.io"))).toEqual([["describe", "yaml", "edit", "copyName", "copyYaml"], ["del"]]);
  });

  it("adds pod-only actions for core Pods", () => {
    expect(ids(kind("Pod"))).toEqual([
      ["describe", "yaml", "edit", "copyName", "copyYaml"],
      ["logs", "shell", "forward"],
      ["del"],
    ]);
  });

  it("does not treat non-core kinds named Pod as pods", () => {
    expect(ids(kind("Pod", "metrics.k8s.io"))).toEqual([["describe", "yaml", "edit", "copyName", "copyYaml"], ["del"]]);
  });

  it("gives Deployments the full workload set including rollout history", () => {
    expect(ids(kind("Deployment", "apps"))).toEqual([
      ["describe", "yaml", "edit", "copyName", "copyYaml"],
      ["logsAll", "viewPods", "scale", "restart", "history"],
      ["del"],
    ]);
  });

  it("gives StatefulSets scale and restart but no history", () => {
    expect(ids(kind("StatefulSet", "apps"))).toEqual([
      ["describe", "yaml", "edit", "copyName", "copyYaml"],
      ["logsAll", "viewPods", "scale", "restart"],
      ["del"],
    ]);
  });

  it("gives DaemonSets restart but not scale", () => {
    expect(ids(kind("DaemonSet", "apps"))).toEqual([
      ["describe", "yaml", "edit", "copyName", "copyYaml"],
      ["logsAll", "viewPods", "restart"],
      ["del"],
    ]);
  });

  it("gives ReplicaSets scale but not restart", () => {
    expect(ids(kind("ReplicaSet", "apps"))).toEqual([
      ["describe", "yaml", "edit", "copyName", "copyYaml"],
      ["logsAll", "viewPods", "scale"],
      ["del"],
    ]);
  });

  it("gives Jobs pod logs and view pods", () => {
    expect(ids(kind("Job", "batch"))).toEqual([
      ["describe", "yaml", "edit", "copyName", "copyYaml"],
      ["logsAll", "viewPods"],
      ["del"],
    ]);
  });

  it("gives CronJobs trigger, suspend, resume, view pods", () => {
    expect(ids(kind("CronJob", "batch"))).toEqual([
      ["describe", "yaml", "edit", "copyName", "copyYaml"],
      ["trigger", "suspend", "resume", "viewPods"],
      ["del"],
    ]);
  });

  it("gives Nodes cordon, uncordon, drain", () => {
    expect(ids(kind("Node"))).toEqual([
      ["describe", "yaml", "edit", "copyName", "copyYaml"],
      ["cordon", "uncordon", "drain"],
      ["del"],
    ]);
  });

  it("gives core Services port-forward", () => {
    expect(ids(kind("Service"))).toEqual([
      ["describe", "yaml", "edit", "copyName", "copyYaml"],
      ["forward"],
      ["del"],
    ]);
  });

  it("gives Secrets and ConfigMaps copy value", () => {
    for (const k of ["Secret", "ConfigMap"]) {
      expect(ids(kind(k))).toEqual([["describe", "yaml", "edit", "copyName", "copyYaml"], ["values"], ["del"]]);
    }
  });

  it("marks only delete as dangerous on pods, delete and drain on nodes", () => {
    const podDanger = menuItemsFor(kind("Pod")).flat().filter((i) => i.danger);
    expect(podDanger.map((i) => i.id)).toEqual(["del"]);
    const nodeDanger = menuItemsFor(kind("Node")).flat().filter((i) => i.danger);
    expect(nodeDanger.map((i) => i.id)).toEqual(["drain", "del"]);
  });
});
