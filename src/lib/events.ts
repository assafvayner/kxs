/** Helpers for the events view: server-side Event Table rows are unordered. */

const DURATION_UNITS: Record<string, number> = { s: 1, m: 60, h: 3600, d: 86400, y: 31536000 };

/**
 * Seconds encoded by a kubectl HumanDuration string ("0s", "45s", "5m30s",
 * "3h", "2d4h", "1y20d"). Anything else — including the API server's
 * "<unknown>" / "<invalid>" placeholders — is null.
 */
export function parseHumanDuration(text: string): number | null {
  const t = text.trim();
  if (!t) return null;
  const re = /(\d+)([smhdy])/g;
  let total = 0;
  let consumed = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(t)) !== null) {
    if (m.index !== consumed) return null; // stray text before this part
    consumed = m.index + m[0].length;
    total += Number(m[1]) * DURATION_UNITS[m[2]];
  }
  if (consumed !== t.length) return null;
  return total;
}

/** Index of a Table column by (case-insensitive) name, or -1. */
export function columnIndex(columns: string[], name: string): number {
  const want = name.trim().toLowerCase();
  return columns.findIndex((c) => c.trim().toLowerCase() === want);
}

export interface EventRowLike {
  created: string | null;
  cells: string[];
}

/**
 * Absolute epoch ms to order an event by: its creationTimestamp when parsable,
 * else `nowMs` minus the Last Seen duration. Unusable rows sort last.
 */
export function eventTimeMs(row: EventRowLike, lastSeenIndex: number, nowMs: number): number {
  if (row.created) {
    const t = Date.parse(row.created);
    if (!Number.isNaN(t)) return t;
  }
  const cell = lastSeenIndex >= 0 ? row.cells[lastSeenIndex] : undefined;
  const secs = cell === undefined ? null : parseHumanDuration(cell);
  return secs === null ? Number.NEGATIVE_INFINITY : nowMs - secs * 1000;
}

/** Newest first. Stable, so equal timestamps keep the server's relative order. */
export function sortEventsNewestFirst<T extends EventRowLike>(
  rows: T[],
  lastSeenIndex: number,
  nowMs: number,
): T[] {
  return rows
    .map((r) => [r, eventTimeMs(r, lastSeenIndex, nowMs)] as const)
    .sort((a, b) => (a[1] === b[1] ? 0 : a[1] > b[1] ? -1 : 1))
    .map(([r]) => r);
}

/** Text the `/` filter matches against: reason + object + message. */
export function eventFilterText(row: EventRowLike, indices: number[]): string {
  return indices
    .filter((i) => i >= 0)
    .map((i) => row.cells[i] ?? "")
    .join(" ");
}

/** Warning stands out, Normal recedes, anything unexpected is flagged. */
export function eventTypeClass(type: string): string {
  const t = type.trim().toLowerCase();
  if (t === "warning") return "st-bad";
  if (t === "normal") return "dim";
  return "st-warn";
}
