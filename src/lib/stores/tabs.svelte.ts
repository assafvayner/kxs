export interface Tab {
  id: number;
  context: string;
}

export class TabsStore {
  tabs = $state<Tab[]>([]);
  /** null = home / launch screen; "settings" = settings pane */
  activeId = $state<number | "settings" | null>(null);
  #nextId = 1;

  open(context: string): void {
    const tab = { id: this.#nextId++, context };
    this.tabs.push(tab);
    this.activeId = tab.id;
  }

  close(id: number): void {
    const i = this.tabs.findIndex((t) => t.id === id);
    if (i === -1) return;
    this.tabs.splice(i, 1);
    if (this.activeId === id) {
      this.activeId = this.tabs.length
        ? this.tabs[Math.min(i, this.tabs.length - 1)].id
        : null;
    }
  }

  activate(id: number | null): void {
    this.activeId = id;
  }

  openSettings(): void {
    this.activeId = "settings";
  }

  activateIndex(i: number): void {
    if (this.tabs[i]) this.activeId = this.tabs[i].id;
  }

  /** ctrl+tab cycling; home (null) is part of the ring */
  cycle(dir: 1 | -1): void {
    if (!this.tabs.length) return;
    const ring: (number | null)[] = [null, ...this.tabs.map((t) => t.id)];
    // "settings" is not in the ring; treat it as home so cycling re-enters the ring.
    const found = ring.indexOf(this.activeId as number | null);
    const cur = found === -1 ? 0 : found;
    this.activeId = ring[(cur + dir + ring.length) % ring.length];
  }
}

export const tabs = new TabsStore();
