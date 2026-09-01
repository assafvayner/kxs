import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ResourceKind } from "./api";
import type { View } from "./stores/viewstack.svelte";
import {
  loadViewMemory,
  namespaceAvailable,
  parseViewMemory,
  rememberedResourceOf,
  resolveRememberedResource,
  saveViewMemory,
  serializeViewMemory,
  topLevelKind,
  viewMemoryKey,
  viewMemoryOf,
  VIEW_MEMORY_VERSION,
} from "./viewMemory";

function mockStorage(): Storage {
  const m = new Map<string, string>();
  return {
    get length() { return m.size; },
    key: (i: number) => [...m.keys()][i] ?? null,
    getItem: (k: string) => (m.has(k) ? m.get(k)! : null),
    setItem: (k: string, v: string) => void m.set(k, String(v)),
    removeItem: (k: string) => void m.delete(k),
    clear: () => m.clear(),
  } as Storage;
}

function kind(over: Partial<ResourceKind> = {}): ResourceKind {
  return {
    group: "autoscaling",
    version: "v2",
    kind: "HorizontalPodAutoscaler",
    plural: "horizontalpodautoscalers",
    namespaced: true,
    aliases: ["hpa"],
    ...over,
  };
}

const HPA = kind();
const POD = kind({ group: "", version: "v1", kind: "Pod", plural: "pods", aliases: ["po"] });

beforeEach(() => {
  vi.stubGlobal("localStorage", mockStorage());
});

describe("viewMemoryKey", () => {
  it("namespaces the key per context", () => {
    expect(viewMemoryKey("infra:hub-prod")).toBe("kxs.viewmemory.infra:hub-prod");
  });
});

describe("rememberedResourceOf", () => {
  it("keeps only the stable identity fields", () => {
    expect(rememberedResourceOf(HPA)).toEqual({
      group: "autoscaling",
      kind: "HorizontalPodAutoscaler",
      plural: "horizontalpodautoscalers",
    });
  });
});

describe("topLevelKind", () => {
  it("returns null for the pods view", () => {
    expect(topLevelKind([{ kind: "pods" }])).toBeNull();
  });

  it("returns the resource kind of a resource view", () => {
    expect(topLevelKind([{ kind: "pods" }, { kind: "resource", resourceKind: HPA }])).toEqual(HPA);
  });

  it("looks through drill-in views to the top-level view below", () => {
    const views: View[] = [
      { kind: "pods" },
      { kind: "resource", resourceKind: HPA },
      { kind: "yaml", title: "HorizontalPodAutoscaler x", body: "" },
      { kind: "logs", namespace: "default", pods: ["a"], label: "a" },
    ];
    expect(topLevelKind(views)).toEqual(HPA);
  });

  it("returns null when the nearest top-level view is pods", () => {
    const views: View[] = [
      { kind: "pods" },
      { kind: "resource", resourceKind: HPA },
      { kind: "pods" },
      { kind: "yaml", title: "Pod x", body: "" },
    ];
    expect(topLevelKind(views)).toBeNull();
  });
});

describe("viewMemoryOf", () => {
  it("records the namespace, resource identity and filter", () => {
    const m = viewMemoryOf("repository-scanner", [{ kind: "resource", resourceKind: HPA }], "-l app=api");
    expect(m).toEqual({
      v: VIEW_MEMORY_VERSION,
      namespace: "repository-scanner",
      resource: rememberedResourceOf(HPA),
      filter: "-l app=api",
    });
  });

  it("records a null resource for the pods view and all-namespaces", () => {
    expect(viewMemoryOf(null, [{ kind: "pods" }], "")).toEqual({
      v: VIEW_MEMORY_VERSION,
      namespace: null,
      resource: null,
      filter: "",
    });
  });
});

describe("parseViewMemory", () => {
  it("round-trips a serialized memory", () => {
    const m = viewMemoryOf("default", [{ kind: "resource", resourceKind: HPA }], "api");
    expect(parseViewMemory(serializeViewMemory(m))).toEqual(m);
  });

  it("returns null for missing data", () => {
    expect(parseViewMemory(null)).toBeNull();
    expect(parseViewMemory("")).toBeNull();
  });

  it("returns null for corrupt JSON", () => {
    expect(parseViewMemory("{not json")).toBeNull();
  });

  it("returns null for a non-object payload", () => {
    expect(parseViewMemory("42")).toBeNull();
    expect(parseViewMemory("null")).toBeNull();
  });

  it("discards a stale version", () => {
    expect(parseViewMemory(JSON.stringify({ v: 0, namespace: null, resource: null, filter: "" }))).toBeNull();
    expect(parseViewMemory(JSON.stringify({ namespace: null, resource: null, filter: "" }))).toBeNull();
  });

  it("rejects a non-string, non-null namespace", () => {
    expect(parseViewMemory(JSON.stringify({ v: 1, namespace: 7, resource: null, filter: "" }))).toBeNull();
    expect(parseViewMemory(JSON.stringify({ v: 1, namespace: "", resource: null, filter: "" }))).toBeNull();
  });

  it("drops a malformed resource but keeps the rest", () => {
    const m = parseViewMemory(JSON.stringify({ v: 1, namespace: "default", resource: { group: "apps" }, filter: "x" }));
    expect(m).toEqual({ v: 1, namespace: "default", resource: null, filter: "x" });
  });

  it("defaults a missing or non-string filter to empty", () => {
    expect(parseViewMemory(JSON.stringify({ v: 1, namespace: null }))?.filter).toBe("");
    expect(parseViewMemory(JSON.stringify({ v: 1, namespace: null, filter: 3 }))?.filter).toBe("");
  });
});

describe("resolveRememberedResource", () => {
  it("resolves against a live kind whose version has drifted", () => {
    const live = kind({ version: "v2beta2" });
    expect(resolveRememberedResource([POD, live], rememberedResourceOf(HPA))).toEqual(live);
  });

  it("falls back to matching group and plural when the kind was renamed", () => {
    const live = kind({ kind: "Hpa" });
    expect(resolveRememberedResource([live], rememberedResourceOf(HPA))).toEqual(live);
  });

  it("returns null for a kind that no longer exists", () => {
    expect(resolveRememberedResource([POD], rememberedResourceOf(HPA))).toBeNull();
    expect(resolveRememberedResource([], rememberedResourceOf(HPA))).toBeNull();
  });

  it("does not match the same kind name in another group", () => {
    const other = kind({ group: "custom.io" });
    expect(resolveRememberedResource([other], rememberedResourceOf(HPA))).toBeNull();
  });
});

describe("namespaceAvailable", () => {
  it("treats all-namespaces as always available", () => {
    expect(namespaceAvailable([], null)).toBe(true);
  });

  it("requires a named namespace to still exist", () => {
    expect(namespaceAvailable(["default", "kube-system"], "default")).toBe(true);
    expect(namespaceAvailable(["default"], "repository-scanner")).toBe(false);
  });
});

describe("loadViewMemory / saveViewMemory", () => {
  it("persists and reloads per context", () => {
    const a = viewMemoryOf("repository-scanner", [{ kind: "resource", resourceKind: HPA }], "");
    saveViewMemory("infra:hub-prod", a);
    expect(loadViewMemory("infra:hub-prod")).toEqual(a);
    expect(loadViewMemory("kind-local")).toBeNull();
  });

  it("returns null on corrupt stored data", () => {
    localStorage.setItem(viewMemoryKey("kind-local"), "{not json");
    expect(loadViewMemory("kind-local")).toBeNull();
  });

  it("survives a throwing storage", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => { throw new Error("denied"); },
      setItem: () => { throw new Error("denied"); },
    } as unknown as Storage);
    expect(loadViewMemory("kind-local")).toBeNull();
    expect(() => saveViewMemory("kind-local", viewMemoryOf(null, [{ kind: "pods" }], ""))).not.toThrow();
  });

  it("returns null when storage is unavailable", () => {
    vi.stubGlobal("localStorage", undefined);
    expect(loadViewMemory("kind-local")).toBeNull();
  });
});
