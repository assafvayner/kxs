import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ColumnWidths,
  MIN_COLUMN_WIDTH,
  buildTemplate,
  clampColumnWidth,
  columnWidthsKey,
  defaultColumnWidths,
  loadColumnWidths,
  parseColumnWidths,
  resourceTableId,
  saveColumnWidths,
  withColumnWidth,
  withoutColumnWidth,
} from "./columnResize.svelte";

function mockStorage(): Storage {
  const m = new Map<string, string>();
  return {
    get length() { return m.size; },
    key: (i: number) => [...m.keys()][i] ?? null,
    getItem: (k: string) => (m.has(k) ? m.get(k)! : null),
    setItem: (k: string, v: string) => void m.set(k, String(v)),
    removeItem: (k: string) => void m.delete(k),
    clear: () => m.clear(),
  } as Storage;
}

beforeEach(() => {
  vi.stubGlobal("localStorage", mockStorage());
});

describe("clampColumnWidth", () => {
  it("keeps a rounded width above the minimum", () => {
    expect(clampColumnWidth(120.4)).toBe(120);
    expect(clampColumnWidth(120.6)).toBe(121);
  });

  it("clamps below the minimum", () => {
    expect(clampColumnWidth(0)).toBe(MIN_COLUMN_WIDTH);
    expect(clampColumnWidth(-500)).toBe(MIN_COLUMN_WIDTH);
  });

  it("honors a custom minimum and rejects non-finite input", () => {
    expect(clampColumnWidth(10, 80)).toBe(80);
    expect(clampColumnWidth(Number.NaN)).toBe(MIN_COLUMN_WIDTH);
  });
});

describe("buildTemplate", () => {
  const defaults = ["1.2fr", "2.4fr", "0.6fr"];

  it("returns the defaults when nothing is resized", () => {
    expect(buildTemplate(defaults, defaultColumnWidths(3))).toBe("1.2fr 2.4fr 0.6fr");
  });

  it("swaps only resized columns to px", () => {
    expect(buildTemplate(defaults, [null, 300, null])).toBe("1.2fr 300px 0.6fr");
  });

  it("falls back to the default track for missing entries", () => {
    expect(buildTemplate(defaults, [200])).toBe("200px 2.4fr 0.6fr");
  });
});

describe("withColumnWidth / withoutColumnWidth", () => {
  it("sets a clamped width without mutating the input", () => {
    const widths = defaultColumnWidths(3);
    expect(withColumnWidth(widths, 1, 10)).toEqual([null, MIN_COLUMN_WIDTH, null]);
    expect(widths).toEqual([null, null, null]);
  });

  it("resets a column to its default", () => {
    expect(withoutColumnWidth([100, 200, 300], 2)).toEqual([100, 200, null]);
  });

  it("ignores out-of-range indexes", () => {
    expect(withColumnWidth([100], 5, 200)).toEqual([100]);
    expect(withoutColumnWidth([100], -1)).toEqual([100]);
  });
});

describe("parseColumnWidths", () => {
  it("defaults on absent or corrupt storage", () => {
    expect(parseColumnWidths(null, 2)).toEqual([null, null]);
    expect(parseColumnWidths("{not json", 2)).toEqual([null, null]);
    expect(parseColumnWidths(JSON.stringify({ a: 1 }), 2)).toEqual([null, null]);
  });

  it("discards widths whose count no longer matches the columns", () => {
    expect(parseColumnWidths(JSON.stringify([100, 200, 300]), 2)).toEqual([null, null]);
  });

  it("clamps stored widths and drops non-numeric entries", () => {
    expect(parseColumnWidths(JSON.stringify([5, "wide", 250]), 3)).toEqual([
      MIN_COLUMN_WIDTH,
      null,
      250,
    ]);
  });
});

describe("persistence", () => {
  it("keys storage per table identity", () => {
    expect(columnWidthsKey("pods")).toBe("kxs.colwidths.pods");
    expect(resourceTableId({ group: "apps", plural: "deployments" })).toBe(
      "res.apps.deployments",
    );
    expect(resourceTableId({ group: "", plural: "services" })).toBe("res..services");
  });

  it("round-trips widths through localStorage", () => {
    saveColumnWidths("pods", [null, 240, null]);
    expect(loadColumnWidths("pods", 3)).toEqual([null, 240, null]);
  });

  it("removes the entry once every column is reset", () => {
    saveColumnWidths("pods", [null, 240, null]);
    saveColumnWidths("pods", [null, null, null]);
    expect(localStorage.getItem(columnWidthsKey("pods"))).toBeNull();
  });

  it("does not leak widths across table identities", () => {
    saveColumnWidths("res.apps.deployments", [120, 120]);
    expect(loadColumnWidths("res..pods", 2)).toEqual([null, null]);
  });

  it("survives a throwing localStorage", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
      removeItem: () => {
        throw new Error("denied");
      },
    } as unknown as Storage);
    expect(loadColumnWidths("pods", 2)).toEqual([null, null]);
    expect(() => saveColumnWidths("pods", [100, 100])).not.toThrow();
  });
});

describe("ColumnWidths", () => {
  it("restores stored widths on configure", () => {
    saveColumnWidths("pods", [null, 300, null]);
    const cw = new ColumnWidths();
    cw.configure("pods", 3);
    expect(cw.template(["1fr", "2fr", "1fr"])).toBe("1fr 300px 1fr");
  });

  it("persists a set width only when committed", () => {
    const cw = new ColumnWidths();
    cw.configure("pods", 2);
    cw.set(0, 150);
    expect(localStorage.getItem(columnWidthsKey("pods"))).toBeNull();
    cw.persist();
    expect(loadColumnWidths("pods", 2)).toEqual([150, null]);
  });

  it("reset clears one column and persists immediately", () => {
    const cw = new ColumnWidths();
    cw.configure("pods", 2);
    cw.set(0, 150);
    cw.set(1, 250);
    cw.persist();
    cw.reset(0);
    expect(loadColumnWidths("pods", 2)).toEqual([null, 250]);
  });

  it("drops stale widths when the column count changes", () => {
    saveColumnWidths("res.apps.deployments", [100, 100, 100]);
    const cw = new ColumnWidths();
    cw.configure("res.apps.deployments", 3);
    expect(cw.widths).toEqual([100, 100, 100]);
    cw.configure("res.apps.deployments", 4);
    expect(cw.widths).toEqual([null, null, null, null]);
  });

  it("reloads when the table identity changes", () => {
    saveColumnWidths("res..pods", [null, 400]);
    const cw = new ColumnWidths();
    cw.configure("res.apps.deployments", 2);
    expect(cw.widths).toEqual([null, null]);
    cw.configure("res..pods", 2);
    expect(cw.widths).toEqual([null, 400]);
  });
});
