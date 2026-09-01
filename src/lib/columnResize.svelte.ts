/** Per-column width in px, or null to keep that column's default track. */
export type ColumnWidth = number | null;

export const MIN_COLUMN_WIDTH = 40;
const KEY_PREFIX = "kxs.colwidths.";

export function columnWidthsKey(tableId: string): string {
  return `${KEY_PREFIX}${tableId}`;
}

export function clampColumnWidth(width: number, min = MIN_COLUMN_WIDTH): number {
  if (!Number.isFinite(width)) return min;
  return Math.max(min, Math.round(width));
}

export function defaultColumnWidths(count: number): ColumnWidth[] {
  return Array.from({ length: count }, () => null);
}

/** Resized columns become fixed px tracks; the rest keep their default track. */
export function buildTemplate(
  defaults: readonly string[],
  widths: readonly ColumnWidth[],
): string {
  return defaults
    .map((d, i) => {
      const w = widths[i];
      return typeof w === "number" ? `${w}px` : d;
    })
    .join(" ");
}

export function withColumnWidth(
  widths: readonly ColumnWidth[],
  index: number,
  width: number,
): ColumnWidth[] {
  if (index < 0 || index >= widths.length) return [...widths];
  const next = [...widths];
  next[index] = clampColumnWidth(width);
  return next;
}

export function withoutColumnWidth(widths: readonly ColumnWidth[], index: number): ColumnWidth[] {
  if (index < 0 || index >= widths.length) return [...widths];
  const next = [...widths];
  next[index] = null;
  return next;
}

/**
 * Stored widths only apply to a table with the same column count; a kind switch
 * or a changed column set discards them rather than mis-assigning tracks.
 */
export function parseColumnWidths(raw: string | null, count: number): ColumnWidth[] {
  if (!raw) return defaultColumnWidths(count);
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return defaultColumnWidths(count);
  }
  if (!Array.isArray(parsed) || parsed.length !== count) return defaultColumnWidths(count);
  return parsed.map((w) => (typeof w === "number" && Number.isFinite(w) ? clampColumnWidth(w) : null));
}

export function loadColumnWidths(tableId: string, count: number): ColumnWidth[] {
  try {
    if (typeof localStorage === "undefined") return defaultColumnWidths(count);
    return parseColumnWidths(localStorage.getItem(columnWidthsKey(tableId)), count);
  } catch {
    return defaultColumnWidths(count);
  }
}

export function saveColumnWidths(tableId: string, widths: readonly ColumnWidth[]): void {
  try {
    if (typeof localStorage === "undefined") return;
    const key = columnWidthsKey(tableId);
    if (widths.every((w) => w === null)) localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(widths));
  } catch {
    /* persistence is best-effort */
  }
}

/** Stable storage identity for a kind's generic resource table. */
export function resourceTableId(kind: { group: string; plural: string }): string {
  return `res.${kind.group}.${kind.plural}`;
}

export class ColumnWidths {
  widths = $state<ColumnWidth[]>([]);
  private tableId = "";

  /** Binds to a table identity + column count, restoring any stored widths. */
  configure(tableId: string, count: number): void {
    if (tableId === this.tableId && this.widths.length === count) return;
    this.tableId = tableId;
    this.widths = loadColumnWidths(tableId, count);
  }

  template(defaults: readonly string[]): string {
    return buildTemplate(defaults, this.widths);
  }

  set(index: number, width: number): void {
    this.widths = withColumnWidth(this.widths, index, width);
  }

  reset(index: number): void {
    this.widths = withoutColumnWidth(this.widths, index);
    this.persist();
  }

  persist(): void {
    saveColumnWidths(this.tableId, this.widths);
  }
}

export interface ColumnDragOptions {
  onwidth: (width: number) => void;
  oncommit?: () => void;
}

/**
 * Tracks the pointer from a handle sitting on the right edge of a header cell,
 * measuring the cell it belongs to so an `fr` track converts to px in place.
 */
export function startColumnDrag(event: PointerEvent, opts: ColumnDragOptions): void {
  const handle = event.currentTarget as HTMLElement | null;
  const cell = handle?.parentElement;
  if (!cell) return;
  event.preventDefault();
  event.stopPropagation();

  const startX = event.clientX;
  const startWidth = cell.getBoundingClientRect().width;
  const body = handle.ownerDocument.body;

  const move = (e: PointerEvent) => opts.onwidth(clampColumnWidth(startWidth + e.clientX - startX));
  const end = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", end);
    window.removeEventListener("pointercancel", end);
    body.classList.remove("col-resizing");
    opts.oncommit?.();
  };

  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", end);
  window.addEventListener("pointercancel", end);
  body.classList.add("col-resizing");
}
