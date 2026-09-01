import { describe, expect, it } from "vitest";
import { DEFAULT_THEME_ID, THEMES, getTheme, xtermTheme } from "./themes";

const HEX = /^#[0-9a-f]{6}$/;
// core tokens must be plain hex (xterm + swatches consume them directly)
const HEX_KEYS = [
  "bg", "bgRaised", "bgHover", "bgActive", "fg", "fgDim",
  "accent", "green", "yellow", "red", "border", "accentFg",
] as const;
// derived tokens may be any non-empty CSS color expression
const CSS_KEYS = ["errorBg", "warnBg", "shadow", "synKey", "synString", "synNumber", "synComment", "synPunct"] as const;

describe("theme registry", () => {
  it("has the default theme", () => {
    expect(THEMES[DEFAULT_THEME_ID]).toBeDefined();
    expect(DEFAULT_THEME_ID).toBe("tokyo-night");
  });

  it("registry keys match theme ids and labels are set", () => {
    for (const [key, t] of Object.entries(THEMES)) {
      expect(t.id).toBe(key);
      expect(t.label.length).toBeGreaterThan(0);
      expect(typeof t.dark).toBe("boolean");
    }
  });

  it("ships 15 themes: 10 dark, 5 light", () => {
    const all = Object.values(THEMES);
    expect(all).toHaveLength(15);
    expect(all.filter((t) => t.dark)).toHaveLength(10);
  });

  it("every theme defines every token with valid values", () => {
    for (const t of Object.values(THEMES)) {
      for (const k of HEX_KEYS) expect(t.colors[k], `${t.id}.${k}`).toMatch(HEX);
      for (const k of CSS_KEYS) expect(t.colors[k].length, `${t.id}.${k}`).toBeGreaterThan(0);
    }
  });

  it("getTheme falls back to the default for unknown ids", () => {
    expect(getTheme("nope").id).toBe(DEFAULT_THEME_ID);
    expect(getTheme("nord").id).toBe("nord");
  });

  it("xtermTheme maps core colors", () => {
    const t = getTheme("tokyo-night");
    expect(xtermTheme(t)).toEqual({
      background: "#16161e",
      foreground: "#c0caf5",
      cursor: "#7aa2f7",
      cursorAccent: "#16161e",
      selectionBackground: "#2f334d",
    });
  });

  it("xtermTheme includes a readable ANSI palette for light themes only", () => {
    const light = xtermTheme(getTheme("github-light"));
    expect(light.white).toBe("#555555");
    expect(light.brightWhite).toBe("#a5a5a5");

    expect("white" in xtermTheme(getTheme("nord"))).toBe(false);
  });
});
