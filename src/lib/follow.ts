export interface ScrollMetrics {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
}

export const DEFAULT_BOTTOM_THRESHOLD_PX = 24;

export function distanceFromBottom(m: ScrollMetrics): number {
  return m.scrollHeight - m.scrollTop - m.clientHeight;
}

export function isNearBottom(m: ScrollMetrics, thresholdPx = DEFAULT_BOTTOM_THRESHOLD_PX): boolean {
  return distanceFromBottom(m) <= thresholdPx;
}

// A scroll event caused by our own code snapping the view to the bottom must
// not be read as the user scrolling away, or every programmatic snap would
// immediately detach following again. Only a scroll we didn't initiate
// should change the following state.
export function nextFollowing(
  current: boolean,
  m: ScrollMetrics,
  opts: { programmatic: boolean; thresholdPx?: number },
): boolean {
  if (opts.programmatic) return current;
  return isNearBottom(m, opts.thresholdPx);
}
