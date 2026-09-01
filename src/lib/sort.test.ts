import { describe, expect, it } from "vitest";
import {
  ageKey,
  compareCells,
  cycleSort,
  isEmptyCell,
  sortBy,
  sortByNumber,
  sortIndicator,
  sortPods,
  sortRows,
} from "./sort";
import type { PodRow, ResourceRow } from "./api";

const order = (cells: string[]) => cells.slice().sort(compareCells);

describe("compareCells", () => {
  it("compares plain numbers numerically, not lexically", () => {
    expect(order(["10", "2", "1"])).toEqual(["1", "2", "10"]);
    expect(compareCells("2", "10")).toBeLessThan(0);
  });

  it("compares k8s memory quantities across suffixes", () => {
    expect(order(["1Gi", "128Mi", "512Mi", "2Ki"])).toEqual(["2Ki", "128Mi", "512Mi", "1Gi"]);
  });

  it("compares cpu quantities with the milli suffix", () => {
    expect(order(["1", "250m", "2500m"])).toEqual(["250m", "1", "2500m"]);
  });

  it("sorts empty-ish cells last in both argument orders", () => {
    for (const empty of ["", "  ", "-", "—", "<none>", "<unknown>"]) {
      expect(compareCells(empty, "web")).toBeGreaterThan(0);
      expect(compareCells("web", empty)).toBeLessThan(0);
    }
    expect(compareCells("", "<none>")).toBe(0);
  });

  it("falls back to case-insensitive string compare", () => {
    expect(compareCells("Web", "api")).toBeGreaterThan(0);
    expect(compareCells("web", "WEB")).toBe(0);
    expect(order(["Running", "completed", "Pending"])).toEqual([
      "completed",
      "Pending",
      "Running",
    ]);
  });

  it("uses the leading number when the cell has a trailing remainder", () => {
    expect(order(["10 (5m ago)", "2 (1h ago)"])).toEqual(["2 (1h ago)", "10 (5m ago)"]);
    expect(order(["0/1", "2/2", "10/10"])).toEqual(["0/1", "2/2", "10/10"]);
  });

  it("breaks leading-number ties on the remainder", () => {
    expect(compareCells("2/2", "2/3")).toBeLessThan(0);
  });

  it("does not treat dotted values as quantities", () => {
    // an IP is not a quantity; leading-octet numeric then string tail
    expect(order(["10.0.0.1", "9.0.0.1"])).toEqual(["9.0.0.1", "10.0.0.1"]);
  });

  it("treats an unknown suffix as a plain string tail", () => {
    expect(order(["90s", "3s"])).toEqual(["3s", "90s"]);
  });
});

describe("isEmptyCell", () => {
  it("recognizes placeholders and blanks only", () => {
    expect(isEmptyCell("<none>")).toBe(true);
    expect(isEmptyCell(" ")).toBe(true);
    expect(isEmptyCell("0")).toBe(false);
  });
});

describe("sortBy", () => {
  const rows = [{ v: "b" }, { v: "" }, { v: "a" }, { v: "c" }];

  it("sorts ascending with empties last", () => {
    expect(sortBy(rows, (r) => r.v, "asc").map((r) => r.v)).toEqual(["a", "b", "c", ""]);
  });

  it("sorts descending and still keeps empties last", () => {
    expect(sortBy(rows, (r) => r.v, "desc").map((r) => r.v)).toEqual(["c", "b", "a", ""]);
  });

  it("does not mutate the input", () => {
    sortBy(rows, (r) => r.v, "asc");
    expect(rows.map((r) => r.v)).toEqual(["b", "", "a", "c"]);
  });

  it("is stable for equal cells", () => {
    const eq = [
      { v: "same", i: 0 },
      { v: "same", i: 1 },
      { v: "same", i: 2 },
    ];
    expect(sortBy(eq, (r) => r.v, "desc").map((r) => r.i)).toEqual([0, 1, 2]);
  });
});

describe("sortByNumber", () => {
  const rows = [{ n: 3 }, { n: null }, { n: 10 }, { n: 1 }];

  it("sorts numerically with nulls last in both directions", () => {
    expect(sortByNumber(rows, (r) => r.n, "asc").map((r) => r.n)).toEqual([1, 3, 10, null]);
    expect(sortByNumber(rows, (r) => r.n, "desc").map((r) => r.n)).toEqual([10, 3, 1, null]);
  });
});

describe("ageKey", () => {
  it("orders youngest first when sorted ascending", () => {
    const older = ageKey("2026-01-01T00:00:00Z")!;
    const newer = ageKey("2026-06-01T00:00:00Z")!;
    expect(newer).toBeLessThan(older);
  });

  it("returns null for missing or unparseable timestamps", () => {
    expect(ageKey(null)).toBeNull();
    expect(ageKey("not-a-date")).toBeNull();
  });
});

describe("cycleSort", () => {
  it("cycles none → asc → desc → none on the same key", () => {
    const asc = cycleSort<number>(null, 1);
    expect(asc).toEqual({ key: 1, dir: "asc" });
    const desc = cycleSort(asc, 1);
    expect(desc).toEqual({ key: 1, dir: "desc" });
    expect(cycleSort(desc, 1)).toBeNull();
  });

  it("restarts at asc when a different key is clicked", () => {
    expect(cycleSort({ key: 1, dir: "desc" }, 2)).toEqual({ key: 2, dir: "asc" });
  });
});

describe("sortIndicator", () => {
  it("marks only the sorted key", () => {
    expect(sortIndicator({ key: "name", dir: "asc" }, "name")).toBe("▲");
    expect(sortIndicator({ key: "name", dir: "desc" }, "name")).toBe("▼");
    expect(sortIndicator({ key: "name", dir: "asc" }, "age")).toBe("");
    expect(sortIndicator(null, "name")).toBe("");
  });
});

function row(name: string, cells: string[], created: string | null): ResourceRow {
  return { key: `default/${name}`, name, namespace: "default", cells, created };
}

describe("sortRows", () => {
  // columns: Name, Replicas, Age (Age is synthetic, index === cells.length)
  const rows = [
    row("b", ["b", "10"], "2026-01-01T00:00:00Z"),
    row("a", ["a", "2"], "2026-06-01T00:00:00Z"),
    row("c", ["c", ""], "2026-03-01T00:00:00Z"),
  ];

  it("sorts by a data column numerically", () => {
    expect(sortRows(rows, 1, "asc").map((r) => r.name)).toEqual(["a", "b", "c"]);
    expect(sortRows(rows, 1, "desc").map((r) => r.name)).toEqual(["b", "a", "c"]);
  });

  it("sorts by the Age column using created, not the rendered string", () => {
    // asc = youngest first
    expect(sortRows(rows, 2, "asc").map((r) => r.name)).toEqual(["a", "c", "b"]);
    expect(sortRows(rows, 2, "desc").map((r) => r.name)).toEqual(["b", "c", "a"]);
  });

  it("treats any out-of-range column as the Age column", () => {
    expect(sortRows(rows, 9, "asc").map((r) => r.name)).toEqual(["a", "c", "b"]);
  });

  it("handles an empty row set", () => {
    expect(sortRows([], 0, "asc")).toEqual([]);
  });
});

function pod(p: Partial<PodRow> & { name: string }): PodRow {
  return {
    key: `default/${p.name}`,
    name: p.name,
    namespace: p.namespace ?? "default",
    ready: p.ready ?? "1/1",
    status: p.status ?? "Running",
    restarts: p.restarts ?? 0,
    ip: p.ip ?? null,
    node: p.node ?? null,
    created: p.created ?? null,
  };
}

describe("sortPods", () => {
  const pods = [
    pod({ name: "web-2", restarts: 10, node: "n2", created: "2026-01-01T00:00:00Z" }),
    pod({ name: "api", restarts: 2, node: null, created: "2026-06-01T00:00:00Z" }),
    pod({ name: "web-1", restarts: 7, node: "n1", created: "2026-03-01T00:00:00Z" }),
  ];

  it("sorts restarts numerically", () => {
    expect(sortPods(pods, "restarts", "asc").map((p) => p.restarts)).toEqual([2, 7, 10]);
    expect(sortPods(pods, "restarts", "desc").map((p) => p.restarts)).toEqual([10, 7, 2]);
  });

  it("sorts age by created (asc = youngest first)", () => {
    expect(sortPods(pods, "age", "asc").map((p) => p.name)).toEqual(["api", "web-1", "web-2"]);
  });

  it("sorts by name", () => {
    expect(sortPods(pods, "name", "asc").map((p) => p.name)).toEqual(["api", "web-1", "web-2"]);
  });

  it("keeps pods without a node last when sorting by node", () => {
    expect(sortPods(pods, "node", "desc").map((p) => p.node)).toEqual(["n2", "n1", null]);
  });

  it("does not mutate the input", () => {
    sortPods(pods, "restarts", "desc");
    expect(pods.map((p) => p.name)).toEqual(["web-2", "api", "web-1"]);
  });
});
