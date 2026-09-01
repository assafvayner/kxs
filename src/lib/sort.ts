import type { PodRow, ResourceRow } from "./api";

export type SortDir = "asc" | "desc";
export interface Sort<K> {
  key: K;
  dir: SortDir;
}

/** Renderings of "no value" used by the apiserver's Table printer and by us. */
const EMPTY_CELLS = new Set(["", "-", "—", "<none>", "<unknown>", "<invalid>"]);

export function isEmptyCell(v: string): boolean {
  return EMPTY_CELLS.has(v.trim());
}

const QUANTITY_UNITS: Record<string, number> = {
  n: 1e-9,
  u: 1e-6,
  µ: 1e-6,
  m: 1e-3,
  k: 1e3,
  K: 1e3,
  M: 1e6,
  G: 1e9,
  T: 1e12,
  P: 1e15,
  E: 1e18,
  Ki: 1024,
  Mi: 1024 ** 2,
  Gi: 1024 ** 3,
  Ti: 1024 ** 4,
  Pi: 1024 ** 5,
  Ei: 1024 ** 6,
};

const NUMBER = String.raw`[+-]?(?:\d+(?:\.\d+)?|\.\d+)`;
const QUANTITY_RE = new RegExp(`^(${NUMBER})\\s*([a-zA-Zµ]{0,2})$`);
const LEADING_RE = new RegExp(`^(${NUMBER})`);

/** The whole cell as a k8s quantity ("3", "250m", "128Mi"), else null. */
function quantity(v: string): number | null {
  const m = QUANTITY_RE.exec(v.trim());
  if (!m) return null;
  const unit = m[2];
  if (unit && !(unit in QUANTITY_UNITS)) return null;
  return Number(m[1]) * (unit ? QUANTITY_UNITS[unit] : 1);
}

/** Leading number plus whatever follows it ("3 (5m ago)" → 3, " (5m ago)"). */
function leading(v: string): { value: number; rest: string } | null {
  const t = v.trim();
  const m = LEADING_RE.exec(t);
  return m === null ? null : { value: Number(m[1]), rest: t.slice(m[1].length) };
}

/**
 * Numeric-aware cell order: quantities and leading numbers compare numerically
 * ("2" < "10", "250m" < "1", "128Mi" < "1Gi"), empty cells sort last, anything
 * else compares case-insensitively.
 */
export function compareCells(a: string, b: string): number {
  const ea = isEmptyCell(a);
  const eb = isEmptyCell(b);
  if (ea || eb) return ea && eb ? 0 : ea ? 1 : -1;
  const qa = quantity(a);
  const qb = quantity(b);
  if (qa !== null && qb !== null) return qa === qb ? 0 : qa < qb ? -1 : 1;
  const la = leading(a);
  const lb = leading(b);
  if (la !== null && lb !== null) {
    if (la.value !== lb.value) return la.value < lb.value ? -1 : 1;
    return la.rest.localeCompare(lb.rest, undefined, { sensitivity: "base" });
  }
  return a.trim().localeCompare(b.trim(), undefined, { sensitivity: "base" });
}

/** Sort by a string cell. Empty cells stay last in both directions. */
export function sortBy<T>(rows: T[], cell: (r: T) => string, dir: SortDir): T[] {
  const sign = dir === "desc" ? -1 : 1;
  return rows.slice().sort((x, y) => {
    const a = cell(x);
    const b = cell(y);
    if (isEmptyCell(a) || isEmptyCell(b)) return compareCells(a, b);
    return sign * compareCells(a, b);
  });
}

/** Sort by a number. Missing values stay last in both directions. */
export function sortByNumber<T>(rows: T[], value: (r: T) => number | null, dir: SortDir): T[] {
  const sign = dir === "desc" ? -1 : 1;
  return rows.slice().sort((x, y) => {
    const a = value(x);
    const b = value(y);
    if (a === null || b === null) return a === b ? 0 : a === null ? 1 : -1;
    return a === b ? 0 : sign * (a < b ? -1 : 1);
  });
}

/**
 * Sort key for an Age column: ascending *age* means youngest first, so the key
 * is the negated creation instant. Absent/unparseable → null (sorts last).
 */
export function ageKey(created: string | null): number | null {
  if (!created) return null;
  const t = Date.parse(created);
  return Number.isNaN(t) ? null : -t;
}

/** Header click cycle: none → asc → desc → none for `key`. */
export function cycleSort<K>(cur: Sort<K> | null, key: K): Sort<K> | null {
  if (cur === null || cur.key !== key) return { key, dir: "asc" };
  return cur.dir === "asc" ? { key, dir: "desc" } : null;
}

export function sortIndicator<K>(cur: Sort<K> | null, key: K): string {
  if (cur === null || cur.key !== key) return "";
  return cur.dir === "asc" ? "▲" : "▼";
}

/**
 * Server-side table rows by column index. The trailing synthetic Age column
 * (index === cells.length) sorts by the creation timestamp, not its rendering.
 */
export function sortRows(rows: ResourceRow[], col: number, dir: SortDir): ResourceRow[] {
  const cellCount = rows[0]?.cells.length ?? 0;
  if (col >= cellCount) return sortByNumber(rows, (r) => ageKey(r.created), dir);
  return sortBy(rows, (r) => r.cells[col] ?? "", dir);
}

export type PodField =
  | "namespace"
  | "name"
  | "ready"
  | "status"
  | "restarts"
  | "ip"
  | "node"
  | "age";

export function sortPods(pods: PodRow[], field: PodField, dir: SortDir): PodRow[] {
  switch (field) {
    case "restarts":
      return sortByNumber(pods, (p) => p.restarts, dir);
    case "age":
      return sortByNumber(pods, (p) => ageKey(p.created), dir);
    case "ip":
      return sortBy(pods, (p) => p.ip ?? "", dir);
    case "node":
      return sortBy(pods, (p) => p.node ?? "", dir);
    default:
      return sortBy(pods, (p) => p[field], dir);
  }
}
