/** k9s-style age: 45s, 2m30s, 2h2m, 2d4h, 60d. Future/invalid → 0s / —. */
export function age(created: string | null, nowMs: number): string {
  if (!created) return "—";
  const t = Date.parse(created);
  if (Number.isNaN(t)) return "—";
  let s = Math.max(0, Math.floor((nowMs - t) / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  s -= m * 60;
  if (m < 60) return `${m}m${s}s`;
  const h = Math.floor(m / 60);
  const mm = m - h * 60;
  if (h < 24) return `${h}h${mm}m`;
  const d = Math.floor(h / 24);
  const hh = h - d * 24;
  if (d < 30) return `${d}d${hh}h`;
  return `${d}d`;
}
