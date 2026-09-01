import type { ITheme } from "@xterm/xterm";

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

type Core = Pick<
  ThemeColors,
  "bg" | "bgRaised" | "bgHover" | "bgActive" | "fg" | "fgDim" | "accent" | "green" | "yellow" | "red" | "border"
>;

/** Derives the secondary tokens from the core palette; overrides win. */
function makeTheme(
  id: string,
  label: string,
  dark: boolean,
  core: Core,
  overrides: Partial<ThemeColors> = {},
): Theme {
  return {
    id,
    label,
    dark,
    colors: {
      ...core,
      accentFg: core.bg,
      errorBg: `color-mix(in srgb, ${core.red} 18%, ${core.bg})`,
      warnBg: `color-mix(in srgb, ${core.yellow} 22%, ${core.bg})`,
      shadow: dark ? "rgba(0, 0, 0, 0.5)" : "rgba(0, 0, 0, 0.18)",
      synKey: core.accent,
      synString: core.green,
      synNumber: core.yellow,
      synComment: core.fgDim,
      synPunct: core.fgDim,
      ...overrides,
    },
  };
}

export const DEFAULT_THEME_ID = "tokyo-night";

const ALL: Theme[] = [
  // -- dark --
  makeTheme("tokyo-night", "Tokyo Night", true, {
    bg: "#16161e", bgRaised: "#1a1b26", bgHover: "#24283b", bgActive: "#2f334d",
    fg: "#c0caf5", fgDim: "#565f89", accent: "#7aa2f7",
    green: "#9ece6a", yellow: "#e0af68", red: "#f7768e", border: "#2f334d",
  }),
  makeTheme("darcula", "Darcula", true, {
    bg: "#2b2b2b", bgRaised: "#3c3f41", bgHover: "#4b4b4b", bgActive: "#4e5254",
    fg: "#a9b7c6", fgDim: "#808080", accent: "#6897bb",
    green: "#6a8759", yellow: "#ffc66d", red: "#ff6b68", border: "#4b4b4b",
  }),
  makeTheme("vscode-dark-plus", "VS Code Dark+", true, {
    bg: "#1e1e1e", bgRaised: "#252526", bgHover: "#2a2d2e", bgActive: "#37373d",
    fg: "#d4d4d4", fgDim: "#808080", accent: "#569cd6",
    green: "#4ec9b0", yellow: "#dcdcaa", red: "#f44747", border: "#3c3c3c",
  }),
  makeTheme("catppuccin-mocha", "Catppuccin Mocha", true, {
    bg: "#1e1e2e", bgRaised: "#181825", bgHover: "#313244", bgActive: "#45475a",
    fg: "#cdd6f4", fgDim: "#6c7086", accent: "#89b4fa",
    green: "#a6e3a1", yellow: "#f9e2af", red: "#f38ba8", border: "#313244",
  }),
  makeTheme("gruvbox-dark", "Gruvbox Dark", true, {
    bg: "#282828", bgRaised: "#1d2021", bgHover: "#3c3836", bgActive: "#504945",
    fg: "#ebdbb2", fgDim: "#928374", accent: "#83a598",
    green: "#b8bb26", yellow: "#fabd2f", red: "#fb4934", border: "#3c3836",
  }),
  makeTheme("nord", "Nord", true, {
    bg: "#2e3440", bgRaised: "#292e39", bgHover: "#3b4252", bgActive: "#434c5e",
    fg: "#d8dee9", fgDim: "#616e88", accent: "#88c0d0",
    green: "#a3be8c", yellow: "#ebcb8b", red: "#bf616a", border: "#3b4252",
  }),
  makeTheme("dracula", "Dracula", true, {
    bg: "#282a36", bgRaised: "#21222c", bgHover: "#343746", bgActive: "#44475a",
    fg: "#f8f8f2", fgDim: "#6272a4", accent: "#bd93f9",
    green: "#50fa7b", yellow: "#f1fa8c", red: "#ff5555", border: "#343746",
  }),
  makeTheme("one-dark", "One Dark", true, {
    bg: "#282c34", bgRaised: "#21252b", bgHover: "#2c313c", bgActive: "#3e4451",
    fg: "#abb2bf", fgDim: "#5c6370", accent: "#61afef",
    green: "#98c379", yellow: "#e5c07b", red: "#e06c75", border: "#2c313c",
  }),
  makeTheme("solarized-dark", "Solarized Dark", true, {
    bg: "#002b36", bgRaised: "#003644", bgHover: "#073642", bgActive: "#0a4a5a",
    fg: "#93a1a1", fgDim: "#586e75", accent: "#268bd2",
    green: "#859900", yellow: "#b58900", red: "#dc322f", border: "#073642",
  }),
  makeTheme("blue-kubernetes", "Blue Kubernetes", true, {
    bg: "#141c2e", bgRaised: "#19233a", bgHover: "#233052", bgActive: "#2e3f6b",
    fg: "#dbe4f5", fgDim: "#7488b3", accent: "#6c9bf2",
    green: "#5fd39a", yellow: "#f0c05c", red: "#f47c8b", border: "#28355c",
  }),
  // -- light --
  makeTheme("catppuccin-latte", "Catppuccin Latte", false, {
    bg: "#eff1f5", bgRaised: "#e6e9ef", bgHover: "#dce0e8", bgActive: "#ccd0da",
    fg: "#4c4f69", fgDim: "#8c8fa1", accent: "#1e66f5",
    green: "#40a02b", yellow: "#df8e1d", red: "#d20f39", border: "#ccd0da",
  }),
  makeTheme("vscode-light", "VS Code Light", false, {
    bg: "#ffffff", bgRaised: "#f3f3f3", bgHover: "#e8e8e8", bgActive: "#e4e6f1",
    fg: "#333333", fgDim: "#717171", accent: "#0066bf",
    green: "#008000", yellow: "#a06a00", red: "#cd3131", border: "#e5e5e5",
  }),
  makeTheme("solarized-light", "Solarized Light", false, {
    bg: "#fdf6e3", bgRaised: "#f5efdc", bgHover: "#eee8d5", bgActive: "#e4ddc8",
    fg: "#657b83", fgDim: "#93a1a1", accent: "#268bd2",
    green: "#859900", yellow: "#b58900", red: "#dc322f", border: "#e4ddc8",
  }),
  makeTheme("github-light", "GitHub Light", false, {
    bg: "#ffffff", bgRaised: "#f6f8fa", bgHover: "#eaeef2", bgActive: "#dde2e8",
    fg: "#1f2328", fgDim: "#656d76", accent: "#0969da",
    green: "#1a7f37", yellow: "#9a6700", red: "#cf222e", border: "#d0d7de",
  }),
  makeTheme("gruvbox-light", "Gruvbox Light", false, {
    bg: "#fbf1c7", bgRaised: "#f2e5bc", bgHover: "#ebdbb2", bgActive: "#d5c4a1",
    fg: "#3c3836", fgDim: "#7c6f64", accent: "#076678",
    green: "#79740e", yellow: "#b57614", red: "#9d0006", border: "#d5c4a1",
  }),
];

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
