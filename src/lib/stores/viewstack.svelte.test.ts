import { describe, expect, it } from "vitest";
import { ViewStack } from "./viewstack.svelte";

const podsView = { kind: "pods" as const };
const resourceView = (name: string) => ({
  kind: "resource" as const,
  resourceKind: { group: "apps", version: "v1", kind: name, plural: name.toLowerCase() + "s", namespaced: true, aliases: [] },
});

describe("ViewStack", () => {
  it("starts on the base pods view", () => {
    const v = new ViewStack();
    expect(v.top.kind).toBe("pods");
    expect(v.canPop).toBe(false);
  });

  it("push/pop", () => {
    const v = new ViewStack();
    v.push(resourceView("Deployment"));
    expect(v.top.kind).toBe("resource");
    expect(v.canPop).toBe(true);
    v.pop();
    expect(v.top.kind).toBe("pods");
    expect(v.canPop).toBe(false);
  });

  it("pop on base is a no-op", () => {
    const v = new ViewStack();
    v.pop();
    expect(v.top.kind).toBe("pods");
  });

  it("replaceTop swaps without growing the stack", () => {
    const v = new ViewStack();
    v.push(resourceView("Deployment"));
    v.replaceTop(resourceView("Service"));
    expect(v.depth).toBe(2);
    expect((v.top as { resourceKind: { kind: string } }).resourceKind.kind).toBe("Service");
    v.pop();
    expect(v.top.kind).toBe("pods");
  });

  it("reset clears to base", () => {
    const v = new ViewStack();
    v.push(resourceView("Deployment"));
    v.push({ kind: "yaml", title: "x", body: "y" });
    v.reset();
    expect(v.top.kind).toBe("pods");
    expect(v.depth).toBe(1);
  });
});
