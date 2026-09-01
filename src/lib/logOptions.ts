/** Log window presets shared by the logs view. `seconds: 0` means "all",
 * i.e. no sinceSeconds at all. */
export const SINCE_OPTIONS: { label: string; seconds: number }[] = [
  { label: "5m", seconds: 300 },
  { label: "15m", seconds: 900 },
  { label: "1h", seconds: 3600 },
  { label: "6h", seconds: 21600 },
  { label: "24h", seconds: 86400 },
  { label: "all", seconds: 0 },
];

/** Multi-pod views stream one request per pod, so they keep a smaller tail cap. */
export function tailOptions(multi: boolean): number[] {
  return multi ? [100, 200, 1000] : [100, 1000, 5000];
}

export function defaultTail(multi: boolean): number {
  return multi ? 200 : 1000;
}

/** The API applies tailLines and sinceSeconds together, truncating the requested
 * window to the last N lines, so only one of them may be sent at a time. */
export function logWindow(
  sinceSeconds: number,
  tailLines: number,
): { tailLines?: number; sinceSeconds?: number } {
  return sinceSeconds > 0
    ? { tailLines: undefined, sinceSeconds }
    : { tailLines, sinceSeconds: undefined };
}
