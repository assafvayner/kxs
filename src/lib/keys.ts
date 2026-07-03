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
    if (tabs.activeId !== null) {
      tabs.close(tabs.activeId);
      e.preventDefault();
    }
  } else if (e.key >= "1" && e.key <= "9") {
    tabs.activateIndex(Number(e.key) - 1);
    e.preventDefault();
  }
}
