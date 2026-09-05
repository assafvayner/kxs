import type { ITheme } from "@xterm/xterm";
import palettes from "../../themes.json";

export interface ThemeColors {
  bg: string;
  bgRaised: string;
  bgHover: string;
  bgActive: string;
  fg: string;
  fgDim: string;
  accent: string;
  green: string;
  yellow: string;
  red: string;
  border: string;
  /** text on accent surfaces (button.primary) */
  accentFg: string;
  errorBg: string;
  warnBg: string;
  shadow: string;
  synKey: string;
  synString: string;
  synNumber: string;
  synComment: string;
  synPunct: string;
}

export interface Theme {
  id: string;
  label: string;
  dark: boolean;
  colors: ThemeColors;
}

export const DEFAULT_THEME_ID = "tokyo-night";

/** Palette data lives in /themes.json, shared with the Rust TUI (kxs_core::theme). */
const ALL: Theme[] = palettes;

export const THEMES: Record<string, Theme> = Object.fromEntries(ALL.map((t) => [t.id, t]));

/** Resolution goes through here (not THEMES directly) so user-defined themes can merge in later. */
export function getTheme(id: string): Theme {
  return THEMES[id] ?? THEMES[DEFAULT_THEME_ID];
}

const VAR_NAMES: Record<keyof ThemeColors, string> = {
  bg: "--bg", bgRaised: "--bg-raised", bgHover: "--bg-hover", bgActive: "--bg-active",
  fg: "--fg", fgDim: "--fg-dim", accent: "--accent", green: "--green", yellow: "--yellow",
  red: "--red", border: "--border", accentFg: "--accent-fg", errorBg: "--error-bg",
  warnBg: "--warn-bg", shadow: "--shadow", synKey: "--syn-key", synString: "--syn-string",
  synNumber: "--syn-number", synComment: "--syn-comment", synPunct: "--syn-punct",
};

/** Writes the theme onto the document root. No-op outside a DOM (tests run in node). */
export function applyTheme(theme: Theme): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  for (const [key, cssVar] of Object.entries(VAR_NAMES) as [keyof ThemeColors, string][]) {
    root.style.setProperty(cssVar, theme.colors[key]);
  }
  root.style.setProperty("color-scheme", theme.dark ? "dark" : "light");
}

// xterm's default ANSI palette assumes a dark background; readable set for light themes
const LIGHT_ANSI = {
  black: "#000000",
  red: "#cd3131",
  green: "#00bc00",
  yellow: "#949800",
  blue: "#0451a5",
  magenta: "#bc05bc",
  cyan: "#0598bc",
  white: "#555555",
  brightBlack: "#666666",
  brightRed: "#cd3131",
  brightGreen: "#14ce14",
  brightYellow: "#b5ba00",
  brightBlue: "#0451a5",
  brightMagenta: "#bc05bc",
  brightCyan: "#0598bc",
  brightWhite: "#a5a5a5",
};

/** xterm renders to canvas and cannot use CSS vars; give it concrete colors. */
export function xtermTheme(theme: Theme): ITheme {
  const c = theme.colors;
  return {
    background: c.bg,
    foreground: c.fg,
    cursor: c.accent,
    cursorAccent: c.bg,
    selectionBackground: c.bgActive,
    ...(theme.dark ? {} : LIGHT_ANSI),
  };
}
