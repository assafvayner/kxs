import { describe, expect, it } from "vitest";
import { age } from "./age";

describe("age", () => {
  const now = Date.parse("2026-07-03T12:00:00Z");
  const at = (iso: string) => age(iso, now);
  it("formats k9s-style", () => {
    expect(at("2026-07-03T11:59:15Z")).toBe("45s");
    expect(at("2026-07-03T11:57:30Z")).toBe("2m30s");
    expect(at("2026-07-03T09:58:00Z")).toBe("2h2m");
    expect(at("2026-07-01T08:00:00Z")).toBe("2d4h");
    expect(at("2026-05-04T12:00:00Z")).toBe("60d");
  });
  it("handles edge cases", () => {
    expect(age(null, now)).toBe("—");
    expect(at("2026-07-03T12:00:30Z")).toBe("0s");
  });
});
