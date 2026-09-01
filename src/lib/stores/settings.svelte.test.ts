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

  it("defaults theme to tokyo-night", () => {
    expect(new SettingsStore().theme).toBe("tokyo-night");
  });

  it("persists and reloads the theme", () => {
    const s = new SettingsStore();
    s.setTheme("nord");
    expect(s.theme).toBe("nord");
    expect(JSON.parse(localStorage.getItem("kxs.settings")!).theme).toBe("nord");
    expect(new SettingsStore().theme).toBe("nord");
  });

  it("falls back to default on an unknown stored theme id", () => {
    localStorage.setItem("kxs.settings", JSON.stringify({ theme: "not-a-theme" }));
    expect(new SettingsStore().theme).toBe("tokyo-night");
  });

  it("setTheme ignores unknown ids", () => {
    const s = new SettingsStore();
    s.setTheme("bogus");
    expect(s.theme).toBe("tokyo-night");
  });

  it("keeps vimMode when saving theme and vice versa", () => {
    const s = new SettingsStore();
    s.setVimMode(true);
    s.setTheme("dracula");
    const raw = JSON.parse(localStorage.getItem("kxs.settings")!);
    expect(raw).toEqual({ vimMode: true, theme: "dracula" });
  });

  it("preview overrides effective theme; commit and revert clear it", () => {
    const s = new SettingsStore();
    expect(s.effectiveTheme.id).toBe("tokyo-night");
    s.setPreviewTheme("nord");
    expect(s.effectiveTheme.id).toBe("nord");
    expect(s.theme).toBe("tokyo-night"); // not committed
    s.setPreviewTheme(null); // revert (mouse leave / Esc)
    expect(s.effectiveTheme.id).toBe("tokyo-night");
    s.setPreviewTheme("dracula");
    s.setTheme("dracula"); // commit clears preview
    expect(s.previewTheme).toBeNull();
    expect(s.effectiveTheme.id).toBe("dracula");
  });

  it("previewing a theme does not touch storage", () => {
    const s = new SettingsStore();
    s.setTheme("nord");
    s.setPreviewTheme("dracula");
    expect(JSON.parse(localStorage.getItem("kxs.settings")!)).toEqual({ vimMode: false, theme: "nord" });
  });
});
