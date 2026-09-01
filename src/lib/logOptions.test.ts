import { describe, expect, it } from "vitest";
import { SINCE_OPTIONS, defaultTail, logWindow, tailOptions } from "./logOptions";

describe("logWindow", () => {
  it("sends only tailLines when since is all", () => {
    expect(logWindow(0, 1000)).toEqual({ tailLines: 1000, sinceSeconds: undefined });
  });

  it("sends only sinceSeconds when a window is chosen", () => {
    expect(logWindow(300, 1000)).toEqual({ tailLines: undefined, sinceSeconds: 300 });
  });
});

describe("tail defaults", () => {
  it("caps multi-pod views lower than single-pod", () => {
    expect(defaultTail(true)).toBeLessThan(defaultTail(false));
  });

  it("offers the default as a selectable option", () => {
    for (const multi of [false, true]) {
      expect(tailOptions(multi)).toContain(defaultTail(multi));
    }
  });
});

describe("SINCE_OPTIONS", () => {
  it("ends with the all preset and has no other zero", () => {
    expect(SINCE_OPTIONS.at(-1)).toEqual({ label: "all", seconds: 0 });
    expect(SINCE_OPTIONS.filter((o) => o.seconds === 0)).toHaveLength(1);
  });
});
