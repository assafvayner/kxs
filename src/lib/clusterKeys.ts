export interface ClusterActions {
  openCommand(): void;
  focusSearch(): void;
  back(): void;
  describe(): void;
  yaml(): void;
  edit(): void;
  copyName(): void;
  copyYaml(): void;
  logs(): void;
  logsAll(): void;
  enter(): void;
  del(): void;
  scale(): void;
  restart(): void;
  history(): void;
  cordon(): void;
  uncordon(): void;
  drain(): void;
  trigger(): void;
  suspend(): void;
  resume(): void;
  viewPods(): void;
  values(): void;
  shell(): void;
  forward(): void;
  /** Move the row selection by delta; returns false when the view has no rows to select. */
  move(delta: number): boolean;
  hasSelection(): boolean;
}

/** Next selected key after moving by `delta` in `keys` (visible row order).
 * No selection (or a stale one) starts from the top/bottom edge; empty list → null. */
export function moveSelection(
  keys: string[],
  selected: string | null,
  delta: number,
): string | null {
  if (keys.length === 0) return null;
  const i = selected === null ? -1 : keys.indexOf(selected);
  if (i === -1) return delta > 0 ? keys[0] : keys[keys.length - 1];
  return keys[Math.min(keys.length - 1, Math.max(0, i + delta))];
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
    case "j":
    case "ArrowDown":
    case "k":
    case "ArrowUp": {
      const handled = a.move(e.key === "j" || e.key === "ArrowDown" ? 1 : -1);
      if (handled) e.preventDefault();
      return handled;
    }
    case ":":
      a.openCommand();
      e.preventDefault();
      return true;
    case "/":
      a.focusSearch();
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
    case "e":
      if (!a.hasSelection()) return false;
      a.edit();
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
    case "u":
      if (!a.hasSelection()) return false;
      a.uncordon();
      e.preventDefault();
      return true;
    case "x":
      if (!a.hasSelection()) return false;
      a.shell();
      e.preventDefault();
      return true;
    case "f":
      if (!a.hasSelection()) return false;
      a.forward();
      e.preventDefault();
      return true;
    default:
      return false;
  }
}

/** tabId → per-tab cluster key handler; App.svelte routes the active tab's key events here. */
export const clusterKeyHandlers = new Map<number, (e: KeyboardEvent) => boolean>();
