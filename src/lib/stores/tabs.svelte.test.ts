import { describe, expect, it } from "vitest";
import { TabsStore } from "./tabs.svelte";

describe("TabsStore", () => {
  it("open appends and activates", () => {
    const t = new TabsStore();
    t.open("prod");
    t.open("dev");
    expect(t.tabs.map((x) => x.context)).toEqual(["prod", "dev"]);
    expect(t.activeId).toBe(t.tabs[1].id);
  });

  it("close active activates the right neighbor, then left at end", () => {
    const t = new TabsStore();
    t.open("a");
    t.open("b");
    t.open("c");
    const b = t.tabs[1].id;
    t.activate(b);
    t.close(b);
    expect(t.activeId).toBe(t.tabs[1].id); // c
    t.close(t.tabs[1].id);
    expect(t.activeId).toBe(t.tabs[0].id); // a
  });

  it("closing the last tab returns home", () => {
    const t = new TabsStore();
    t.open("a");
    t.close(t.tabs[0].id);
    expect(t.activeId).toBeNull();
    expect(t.tabs).toEqual([]);
  });

  it("closing an inactive tab keeps the active one", () => {
    const t = new TabsStore();
    t.open("a");
    t.open("b");
    const a = t.tabs[0].id;
    t.close(a);
    expect(t.tabs[0].context).toBe("b");
    expect(t.activeId).toBe(t.tabs[0].id);
  });

  it("cycle wraps through home in both directions", () => {
    const t = new TabsStore();
    t.open("a");
    t.open("b");
    t.activate(null);
    t.cycle(1);
    expect(t.activeId).toBe(t.tabs[0].id);
    t.cycle(1);
    expect(t.activeId).toBe(t.tabs[1].id);
    t.cycle(1);
    expect(t.activeId).toBeNull();
    t.cycle(-1);
    expect(t.activeId).toBe(t.tabs[1].id);
  });

  it("activateIndex ignores out-of-range", () => {
    const t = new TabsStore();
    t.open("a");
    t.activateIndex(5);
    expect(t.activeId).toBe(t.tabs[0].id);
  });
});
