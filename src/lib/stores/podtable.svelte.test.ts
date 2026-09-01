import { describe, expect, it } from "vitest";
import { PodTable } from "./podtable.svelte";
import type { PodRow } from "../api";

const row = (key: string, status = "Running"): PodRow => ({
  key,
  name: key.split("/")[1],
  namespace: key.split("/")[0],
  ready: "1/1",
  status,
  restarts: 0,
  ip: null,
  node: null,
  created: null,
  cpuRequestMillis: null,
  memRequestMib: null,
});

describe("PodTable", () => {
  it("snapshot replaces and sorts", () => {
    const t = new PodTable();
    t.apply({ type: "upsert", rows: [row("z/old")] });
    t.apply({ type: "snapshot", rows: [row("b/y"), row("a/z"), row("a/x")] });
    expect(t.rows.map((r) => r.key)).toEqual(["a/x", "a/z", "b/y"]);
  });

  it("upsert inserts and replaces", () => {
    const t = new PodTable();
    t.apply({ type: "snapshot", rows: [row("a/x")] });
    t.apply({ type: "upsert", rows: [row("a/x", "CrashLoopBackOff"), row("a/y")] });
    expect(t.rows.map((r) => r.key)).toEqual(["a/x", "a/y"]);
    expect(t.rows[0].status).toBe("CrashLoopBackOff");
  });

  it("delete removes", () => {
    const t = new PodTable();
    t.apply({ type: "snapshot", rows: [row("a/x"), row("a/y")] });
    t.apply({ type: "delete", keys: ["a/x"] });
    expect(t.rows.map((r) => r.key)).toEqual(["a/y"]);
  });

  it("status events are ignored by the table", () => {
    const t = new PodTable();
    t.apply({ type: "snapshot", rows: [row("a/x")] });
    t.apply({ type: "status", state: "reconnecting", message: null });
    expect(t.rows.length).toBe(1);
  });
});
