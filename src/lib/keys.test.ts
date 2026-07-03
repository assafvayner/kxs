import { describe, expect, it, vi } from "vitest";
import { TabsStore } from "./stores/tabs.svelte";
import { handleKeydown, type KeyInput } from "./keys";

const ev = (
  key: string,
  mods: Partial<Pick<KeyInput, "ctrlKey" | "metaKey" | "shiftKey">> = {},
) => ({ key, ctrlKey: false, metaKey: false, shiftKey: false, preventDefault: vi.fn(), ...mods });

describe("handleKeydown", () => {
  it("ctrl+tab cycles forward, ctrl+shift+tab backward", () => {
    const t = new TabsStore();
    t.open("a");
    t.open("b");
    t.activate(null);
    const fwd = ev("Tab", { ctrlKey: true });
    handleKeydown(fwd, t);
    expect(t.activeId).toBe(t.tabs[0].id);
    expect(fwd.preventDefault).toHaveBeenCalled();
    handleKeydown(ev("Tab", { ctrlKey: true, shiftKey: true }), t);
    expect(t.activeId).toBeNull();
  });

  it("cmd+t goes home, cmd+w closes active", () => {
    const t = new TabsStore();
    t.open("a");
    handleKeydown(ev("t", { metaKey: true }), t);
    expect(t.activeId).toBeNull();
    t.activate(t.tabs[0].id);
    handleKeydown(ev("w", { metaKey: true }), t);
    expect(t.tabs).toEqual([]);
  });

  it("cmd+w on home does nothing", () => {
    const t = new TabsStore();
    t.open("a");
    t.activate(null);
    handleKeydown(ev("w", { metaKey: true }), t);
    expect(t.tabs.length).toBe(1);
  });

  it("cmd+digit activates by index", () => {
    const t = new TabsStore();
    t.open("a");
    t.open("b");
    handleKeydown(ev("1", { metaKey: true }), t);
    expect(t.activeId).toBe(t.tabs[0].id);
  });
});
