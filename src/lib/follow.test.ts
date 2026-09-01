import { describe, expect, it } from "vitest";
import { distanceFromBottom, isNearBottom, nextFollowing } from "./follow";

describe("distanceFromBottom", () => {
  it("is zero when scrolled fully to the bottom", () => {
    expect(distanceFromBottom({ scrollTop: 900, clientHeight: 100, scrollHeight: 1000 })).toBe(0);
  });

  it("is positive when scrolled up", () => {
    expect(distanceFromBottom({ scrollTop: 500, clientHeight: 100, scrollHeight: 1000 })).toBe(400);
  });
});

describe("isNearBottom", () => {
  it("treats exact bottom as near", () => {
    expect(isNearBottom({ scrollTop: 900, clientHeight: 100, scrollHeight: 1000 })).toBe(true);
  });

  it("tolerates sub-pixel float rounding within the threshold", () => {
    expect(isNearBottom({ scrollTop: 899.7, clientHeight: 100, scrollHeight: 1000 })).toBe(true);
  });

  it("is false once past the threshold", () => {
    expect(isNearBottom({ scrollTop: 800, clientHeight: 100, scrollHeight: 1000 }, 24)).toBe(false);
  });

  it("respects a custom threshold", () => {
    const m = { scrollTop: 850, clientHeight: 100, scrollHeight: 1000 };
    expect(isNearBottom(m, 50)).toBe(true);
    expect(isNearBottom(m, 40)).toBe(false);
  });
});

describe("nextFollowing", () => {
  const atBottom = { scrollTop: 900, clientHeight: 100, scrollHeight: 1000 };
  const scrolledUp = { scrollTop: 200, clientHeight: 100, scrollHeight: 1000 };

  it("detaches when the user scrolls away from the bottom", () => {
    expect(nextFollowing(true, scrolledUp, { programmatic: false })).toBe(false);
  });

  it("reattaches when the user scrolls back near the bottom", () => {
    expect(nextFollowing(false, atBottom, { programmatic: false })).toBe(true);
  });

  it("ignores a programmatic scroll away from the bottom, keeping current state", () => {
    expect(nextFollowing(true, scrolledUp, { programmatic: true })).toBe(true);
  });

  it("ignores a programmatic scroll to the bottom while detached", () => {
    expect(nextFollowing(false, atBottom, { programmatic: true })).toBe(false);
  });
});
