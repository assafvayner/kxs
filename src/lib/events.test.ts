import { describe, expect, it } from "vitest";
import {
  columnIndex,
  eventFilterText,
  eventTimeMs,
  eventTypeClass,
  parseHumanDuration,
  sortEventsNewestFirst,
} from "./events";

const row = (created: string | null, cells: string[]) => ({ created, cells });

describe("parseHumanDuration", () => {
  it("parses every single-unit form kubectl emits", () => {
    expect(parseHumanDuration("0s")).toBe(0);
    expect(parseHumanDuration("45s")).toBe(45);
    expect(parseHumanDuration("7m")).toBe(420);
    expect(parseHumanDuration("3h")).toBe(10800);
    expect(parseHumanDuration("2d")).toBe(172800);
    expect(parseHumanDuration("1y")).toBe(31536000);
  });
  it("parses two-unit forms", () => {
    expect(parseHumanDuration("5m30s")).toBe(330);
    expect(parseHumanDuration("2h15m")).toBe(8100);
    expect(parseHumanDuration("2d4h")).toBe(187200);
  });
  it("tolerates surrounding whitespace", () => {
    expect(parseHumanDuration("  12m  ")).toBe(720);
  });
  it("rejects placeholders and junk", () => {
    expect(parseHumanDuration("<unknown>")).toBeNull();
    expect(parseHumanDuration("<invalid>")).toBeNull();
    expect(parseHumanDuration("")).toBeNull();
    expect(parseHumanDuration("soon")).toBeNull();
    expect(parseHumanDuration("5x")).toBeNull();
    expect(parseHumanDuration("5m ago")).toBeNull();
    expect(parseHumanDuration("about 5m")).toBeNull();
  });
});

describe("columnIndex", () => {
  const columns = ["Last Seen", "Type", "Reason", "Object", "Message", "Age"];
  it("finds columns case-insensitively", () => {
    expect(columnIndex(columns, "last seen")).toBe(0);
    expect(columnIndex(columns, "MESSAGE")).toBe(4);
  });
  it("returns -1 when absent", () => {
    expect(columnIndex(columns, "subobject")).toBe(-1);
  });
});

describe("eventTimeMs", () => {
  const now = Date.parse("2026-01-01T12:00:00Z");
  it("prefers a parsable creationTimestamp", () => {
    expect(eventTimeMs(row("2026-01-01T11:00:00Z", ["9h"]), 0, now)).toBe(
      Date.parse("2026-01-01T11:00:00Z"),
    );
  });
  it("falls back to the Last Seen cell", () => {
    expect(eventTimeMs(row(null, ["10m"]), 0, now)).toBe(now - 600_000);
    expect(eventTimeMs(row("not-a-date", ["10m"]), 0, now)).toBe(now - 600_000);
  });
  it("sorts last when neither source is usable", () => {
    expect(eventTimeMs(row(null, ["<unknown>"]), 0, now)).toBe(Number.NEGATIVE_INFINITY);
    expect(eventTimeMs(row(null, ["5m"]), -1, now)).toBe(Number.NEGATIVE_INFINITY);
  });
});

describe("sortEventsNewestFirst", () => {
  const now = Date.parse("2026-01-01T12:00:00Z");
  it("orders newest first across both time sources", () => {
    const rows = [
      row(null, ["<unknown>", "oldest-unusable"]),
      row("2026-01-01T09:00:00Z", ["3h", "three-hours"]),
      row(null, ["30s", "thirty-seconds"]),
      row("2026-01-01T11:30:00Z", ["30m", "thirty-minutes"]),
    ];
    expect(sortEventsNewestFirst(rows, 0, now).map((r) => r.cells[1])).toEqual([
      "thirty-seconds",
      "thirty-minutes",
      "three-hours",
      "oldest-unusable",
    ]);
  });
  it("is stable for equal timestamps and does not mutate the input", () => {
    const rows = [row(null, ["1m", "a"]), row(null, ["1m", "b"]), row(null, ["1m", "c"])];
    const sorted = sortEventsNewestFirst(rows, 0, now);
    expect(sorted.map((r) => r.cells[1])).toEqual(["a", "b", "c"]);
    expect(rows.map((r) => r.cells[1])).toEqual(["a", "b", "c"]);
  });
});

describe("eventFilterText", () => {
  it("joins the requested cells and skips missing columns", () => {
    const r = row(null, ["5m", "Warning", "BackOff", "pod/web-1", "Back-off pulling image"]);
    expect(eventFilterText(r, [2, 3, 4])).toBe("BackOff pod/web-1 Back-off pulling image");
    expect(eventFilterText(r, [2, -1, 4])).toBe("BackOff Back-off pulling image");
  });
});

describe("eventTypeClass", () => {
  it("maps the known event types", () => {
    expect(eventTypeClass("Warning")).toBe("st-bad");
    expect(eventTypeClass("normal")).toBe("dim");
    expect(eventTypeClass("Weird")).toBe("st-warn");
    expect(eventTypeClass("")).toBe("st-warn");
  });
});
