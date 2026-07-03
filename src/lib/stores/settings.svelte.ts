export interface Settings {
  vimMode: boolean;
}

const KEY = "kxs.settings";
const DEFAULTS: Settings = { vimMode: false };

function load(): Settings {
  try {
    if (typeof localStorage === "undefined") return { ...DEFAULTS };
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...DEFAULTS };
    const parsed = JSON.parse(raw) as Partial<Settings>;
    return { vimMode: typeof parsed?.vimMode === "boolean" ? parsed.vimMode : DEFAULTS.vimMode };
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
  vimMode = $state<boolean>(load().vimMode);

  setVimMode(v: boolean): void {
    this.vimMode = v;
    save({ vimMode: v });
  }
}

export const settings = new SettingsStore();
