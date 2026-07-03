import type { TabsStore } from "./stores/tabs.svelte";

/** Structural subset of KeyboardEvent so tests don't need a DOM. */
export interface KeyInput {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  preventDefault(): void;
}

export function handleKeydown(e: KeyInput, tabs: TabsStore): void {
  if (e.ctrlKey && e.key === "Tab") {
    tabs.cycle(e.shiftKey ? -1 : 1);
    e.preventDefault();
    return;
  }
  const mod = e.metaKey || e.ctrlKey;
  if (!mod) return;
  if (e.key === "t") {
    tabs.activate(null);
    e.preventDefault();
  } else if (e.key === "w") {
    if (typeof tabs.activeId === "number") {
      tabs.close(tabs.activeId);
      e.preventDefault();
    }
  } else if (e.key >= "1" && e.key <= "9") {
    tabs.activateIndex(Number(e.key) - 1);
    e.preventDefault();
  }
}

/** True when the event target is a typing surface — window-level shortcuts
 * must not fire while the user edits (ctrl+tab cycling excepted at the call site). */
export function isEditableTarget(target: unknown): boolean {
  if (!target || typeof target !== "object") return false;
  const el = target as { tagName?: string; isContentEditable?: boolean };
  return el.isContentEditable === true || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName ?? "");
}
