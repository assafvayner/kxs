import type { PodEvent, PodRow } from "../api";

export class PodTable {
  rows = $state<PodRow[]>([]);
  #map = new Map<string, PodRow>();

  apply(ev: PodEvent): void {
    if (ev.type === "snapshot") {
      this.#map = new Map(ev.rows.map((r) => [r.key, r]));
    } else if (ev.type === "upsert") {
      for (const r of ev.rows) this.#map.set(r.key, r);
    } else if (ev.type === "delete") {
      for (const k of ev.keys) this.#map.delete(k);
    } else {
      return;
    }
    this.rows = [...this.#map.values()].sort((a, b) => a.key.localeCompare(b.key));
  }
}
