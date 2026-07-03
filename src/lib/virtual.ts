export interface WindowRange {
  start: number;
  end: number;
  padTop: number;
  padBottom: number;
}

export function windowRange(
  scrollTop: number,
  viewportHeight: number,
  itemHeight: number,
  itemCount: number,
  overscan = 10,
): WindowRange {
  if (itemCount === 0) return { start: 0, end: 0, padTop: 0, padBottom: 0 };
  const first = Math.floor(scrollTop / itemHeight);
  const last = Math.ceil((scrollTop + viewportHeight) / itemHeight);
  const start = Math.max(0, Math.min(first - overscan, itemCount));
  const end = Math.max(start, Math.min(last + overscan, itemCount));
  return {
    start,
    end,
    padTop: start * itemHeight,
    padBottom: (itemCount - end) * itemHeight,
  };
}
