import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsStore } from "./settings.svelte";

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

beforeEach(() => {
  vi.stubGlobal("localStorage", mockStorage());
});

describe("SettingsStore", () => {
  it("defaults vimMode to false", () => {
    expect(new SettingsStore().vimMode).toBe(false);
  });

  it("loads a persisted vimMode", () => {
    localStorage.setItem("kxs.settings", JSON.stringify({ vimMode: true }));
    expect(new SettingsStore().vimMode).toBe(true);
  });

  it("setVimMode updates and persists", () => {
    const s = new SettingsStore();
    s.setVimMode(true);
    expect(s.vimMode).toBe(true);
    expect(JSON.parse(localStorage.getItem("kxs.settings")!).vimMode).toBe(true);
  });

  it("falls back to defaults on corrupt JSON", () => {
    localStorage.setItem("kxs.settings", "{not json");
    expect(new SettingsStore().vimMode).toBe(false);
  });

  it("falls back to default on a non-boolean vimMode", () => {
    localStorage.setItem("kxs.settings", JSON.stringify({ vimMode: "yes" }));
    expect(new SettingsStore().vimMode).toBe(false);
  });
});
