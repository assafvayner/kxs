import { describe, expect, it } from "vitest";
import { cursorContext } from "./cursor";

/** Places the cursor at the `|` marker and returns text + offset. */
function cursor(src: string): [string, number] {
  const pos = src.indexOf("|");
  return [src.slice(0, pos) + src.slice(pos + 1), pos];
}

describe("cursorContext", () => {
  it("is a root key on an unindented line", () => {
    const c = cursorContext(...cursor("apiVersion: v1\nkind: Pod\nsp|"))!;
    expect(c).toMatchObject({ mode: "key", path: [], word: "sp", siblings: ["apiVersion", "kind"], indent: 0 });
    expect(c.from).toBe("apiVersion: v1\nkind: Pod\n".length);
  });

  it("walks up through parent keys by indentation", () => {
    const c = cursorContext(...cursor("spec:\n  template:\n    metadata:\n      labels:\n        app: web\n    spec:\n      cont|\n"))!;
    expect(c.path).toEqual(["spec", "template", "spec"]);
    expect(c.word).toBe("cont");
    expect(c.indent).toBe(6);
  });

  it("indexes sequence items and treats dash keys as item keys", () => {
    const c = cursorContext(...cursor("spec:\n  containers:\n    - name: a\n      image: x\n    - name: b\n      im|\n"))!;
    expect(c.path).toEqual(["spec", "containers", 1]);
    expect(c.siblings).toEqual(["name"]);
    expect(c.indent).toBe(6);
  });

  it("handles a fresh dash item", () => {
    const c = cursorContext(...cursor("spec:\n  containers:\n    - name: a\n    - |"))!;
    expect(c).toMatchObject({ mode: "key", path: ["spec", "containers", 1], word: "", siblings: [], indent: 6 });
  });

  it("collects siblings below the cursor too", () => {
    const c = cursorContext(...cursor("spec:\n  rep|\n  selector: {}\n  template: {}\n"))!;
    expect(c.siblings).toEqual(["selector", "template"]);
  });

  it("is a value after 'key: '", () => {
    const c = cursorContext(...cursor("spec:\n  containers:\n    - imagePullPolicy: Al|\n"))!;
    expect(c).toMatchObject({ mode: "value", path: ["spec", "containers", 0], key: "imagePullPolicy", word: "Al" });
  });

  it("returns null inside a quoted string or after a value", () => {
    expect(cursorContext(...cursor('metadata:\n  name: "we|b"\n'))).toBeNull();
    expect(cursorContext(...cursor("metadata:\n  name: web ext|\n"))).toBeNull();
  });

  it("ignores comments and blank lines when walking up", () => {
    const c = cursorContext(...cursor("spec:\n  # replicas\n\n  rep|"))!;
    expect(c.path).toEqual(["spec"]);
  });
});
