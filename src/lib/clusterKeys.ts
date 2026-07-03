export interface ClusterActions {
  openCommand(): void;
  openFilter(): void;
  back(): void;
  describe(): void;
  yaml(): void;
  logs(): void;
  enter(): void;
  del(): void;
  scale(): void;
  restart(): void;
  cordon(): void;
  hasSelection(): boolean;
}

export interface ClusterKeyInput {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  preventDefault(): void;
}

/** Returns true if it handled the key (caller should not fall through to tab shortcuts). */
export function handleClusterKey(e: ClusterKeyInput, a: ClusterActions): boolean {
  if (e.ctrlKey && (e.key === "d" || e.key === "D")) {
    if (!a.hasSelection()) return false;
    a.del();
    e.preventDefault();
    return true;
  }
  if (e.metaKey || e.ctrlKey) return false;
  switch (e.key) {
    case ":":
      a.openCommand();
      e.preventDefault();
      return true;
    case "/":
      a.openFilter();
      e.preventDefault();
      return true;
    case "Escape":
      a.back();
      e.preventDefault();
      return true;
    case "Enter":
      if (!a.hasSelection()) return false;
      a.enter();
      e.preventDefault();
      return true;
    case "d":
    case "y":
    case "l":
      if (!a.hasSelection()) return false;
      if (e.key === "d") a.describe();
      else if (e.key === "y") a.yaml();
      else a.logs();
      e.preventDefault();
      return true;
    case "s":
      if (!a.hasSelection()) return false;
      a.scale();
      e.preventDefault();
      return true;
    case "r":
      if (!a.hasSelection()) return false;
      a.restart();
      e.preventDefault();
      return true;
    case "c":
      if (!a.hasSelection()) return false;
      a.cordon();
      e.preventDefault();
      return true;
    default:
      return false;
  }
}

/** tabId → per-tab cluster key handler; App.svelte routes the active tab's key events here. */
export const clusterKeyHandlers = new Map<number, (e: KeyboardEvent) => boolean>();
