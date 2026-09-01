/** Utilization formatting: usage vs. request/allocatable, with thresholds. */

export const NO_VALUE = "—";

/** Percent of `total`, rounded. Null when `total` is unknown or zero. */
export function percent(used: number, total: number | null | undefined): number | null {
  if (total === null || total === undefined || total <= 0) return null;
  return Math.round((used / total) * 100);
}

/** Threshold class for a percentage: >100% bad, >80% warn, otherwise none. */
export function utilClass(pct: number | null): string {
  if (pct === null) return "";
  if (pct > 100) return "st-bad";
  if (pct > 80) return "st-warn";
  return "";
}

export interface Utilization {
  text: string;
  cls: string;
}

function format(used: number | null | undefined, unit: string, total: number | null | undefined) {
  if (used === null || used === undefined) return { text: NO_VALUE, cls: "" };
  const pct = percent(used, total);
  return {
    text: pct === null ? `${used}${unit}` : `${used}${unit} ${pct}%`,
    cls: utilClass(pct),
  };
}

/** CPU cell: "123m", or "123m 49%" when the request/allocatable is known. */
export function cpuUtil(
  usedMillis: number | null | undefined,
  totalMillis: number | null | undefined,
): Utilization {
  return format(usedMillis, "m", totalMillis);
}

/** Memory cell: "45Mi", or "45Mi 35%" when the request/allocatable is known. */
export function memUtil(
  usedMib: number | null | undefined,
  totalMib: number | null | undefined,
): Utilization {
  return format(usedMib, "Mi", totalMib);
}

/** "used/total unit pct%" for the node rows, e.g. "412m/4000m 10%". */
export function ofTotal(
  used: number,
  total: number | null | undefined,
  unit: string,
): Utilization {
  const pct = percent(used, total);
  return {
    text: pct === null ? `${used}${unit}/${NO_VALUE}` : `${used}${unit}/${total}${unit} ${pct}%`,
    cls: utilClass(pct),
  };
}
