import { describe, expect, it } from "vitest";
import { windowRange } from "./virtual";

describe("windowRange", () => {
  it("computes visible slice with overscan", () => {
    const r = windowRange(280, 300, 28, 1000, 5);
    expect(r.start).toBe(5); // floor(280/28)=10, minus overscan 5
    expect(r.end).toBe(26); // ceil((280+300)/28)=21, plus overscan 5
    expect(r.padTop).toBe(5 * 28);
    expect(r.padBottom).toBe((1000 - 26) * 28);
  });

  it("clamps at the top", () => {
    const r = windowRange(0, 300, 28, 1000, 5);
    expect(r.start).toBe(0);
    expect(r.padTop).toBe(0);
  });

  it("clamps at the bottom and for short lists", () => {
    const r = windowRange(99999, 300, 28, 10, 5);
    expect(r.end).toBe(10);
    expect(r.start).toBeLessThanOrEqual(10);
    expect(r.padBottom).toBe(0);
  });

  it("empty list", () => {
    const r = windowRange(0, 300, 28, 0, 5);
    expect(r).toEqual({ start: 0, end: 0, padTop: 0, padBottom: 0 });
  });
});
