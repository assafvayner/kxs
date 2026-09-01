import { DEFAULT_THEME_ID, THEMES, getTheme, type Theme } from "../themes";

export interface Settings {
  vimMode: boolean;
  theme: string;
}

const KEY = "kxs.settings";
const DEFAULTS: Settings = { vimMode: false, theme: DEFAULT_THEME_ID };

function load(): Settings {
  try {
    if (typeof localStorage === "undefined") return { ...DEFAULTS };
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...DEFAULTS };
    const parsed = JSON.parse(raw) as Partial<Settings>;
    return {
      vimMode: typeof parsed?.vimMode === "boolean" ? parsed.vimMode : DEFAULTS.vimMode,
      theme: typeof parsed?.theme === "string" && parsed.theme in THEMES ? parsed.theme : DEFAULTS.theme,
    };
  } catch {
    return { ...DEFAULTS };
  }
}

function save(s: Settings): void {
  try {
    if (typeof localStorage !== "undefined") localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    /* persistence is best-effort */
  }
}

export class SettingsStore {
  vimMode = $state<boolean>(false);
  theme = $state<string>(DEFAULT_THEME_ID);
  /** transient picker preview; never persisted */
  previewTheme = $state<string | null>(null);

  constructor() {
    const initial = load();
    this.vimMode = initial.vimMode;
    this.theme = initial.theme;
  }

  /** what the UI should render right now */
  get effectiveTheme(): Theme {
    return getTheme(this.previewTheme ?? this.theme);
  }

  setVimMode(v: boolean): void {
    this.vimMode = v;
    save({ vimMode: v, theme: this.theme });
  }

  setTheme(id: string): void {
    if (!(id in THEMES)) return;
    this.theme = id;
    this.previewTheme = null;
    save({ vimMode: this.vimMode, theme: id });
  }

  setPreviewTheme(id: string | null): void {
    this.previewTheme = id;
  }
}

export const settings = new SettingsStore();
