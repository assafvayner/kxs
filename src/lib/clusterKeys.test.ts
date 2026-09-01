import { describe, expect, it, vi } from "vitest";
import { handleClusterKey, moveSelection, type ClusterActions } from "./clusterKeys";

function actions(overrides: Partial<ClusterActions> = {}): ClusterActions {
  return {
    openCommand: vi.fn(),
    focusSearch: vi.fn(),
    back: vi.fn(),
    describe: vi.fn(),
    yaml: vi.fn(),
    edit: vi.fn(),
    logs: vi.fn(),
    logsAll: vi.fn(),
    enter: vi.fn(),
    del: vi.fn(),
    scale: vi.fn(),
    restart: vi.fn(),
    history: vi.fn(),
    cordon: vi.fn(),
    uncordon: vi.fn(),
    drain: vi.fn(),
    trigger: vi.fn(),
    suspend: vi.fn(),
    resume: vi.fn(),
    viewPods: vi.fn(),
    values: vi.fn(),
    shell: vi.fn(),
    forward: vi.fn(),
    move: vi.fn(() => true),
    hasSelection: () => true,
    ...overrides,
  };
}
const ev = (key: string, mods: any = {}) => ({ key, ctrlKey: false, metaKey: false, shiftKey: false, preventDefault: vi.fn(), ...mods });

describe("handleClusterKey", () => {
  it(": opens command, / focuses search", () => {
    const a = actions();
    const e1 = ev(":"); expect(handleClusterKey(e1, a)).toBe(true); expect(a.openCommand).toHaveBeenCalled();
    const e2 = ev("/"); expect(handleClusterKey(e2, a)).toBe(true); expect(a.focusSearch).toHaveBeenCalled();
  });
  it("esc pops the view", () => {
    const a = actions();
    handleClusterKey(ev("Escape"), a);
    expect(a.back).toHaveBeenCalled();
  });
  it("d/y/l/enter act on the selection", () => {
    const a = actions();
    handleClusterKey(ev("d"), a); expect(a.describe).toHaveBeenCalled();
    handleClusterKey(ev("y"), a); expect(a.yaml).toHaveBeenCalled();
    handleClusterKey(ev("l"), a); expect(a.logs).toHaveBeenCalled();
    handleClusterKey(ev("Enter"), a); expect(a.enter).toHaveBeenCalled();
  });
  it("d/y/l ignored with no selection", () => {
    const a = actions({ hasSelection: () => false });
    expect(handleClusterKey(ev("d"), a)).toBe(false);
    expect(a.describe).not.toHaveBeenCalled();
  });
  it("modifiers or unhandled keys return false (let tab shortcuts run)", () => {
    const a = actions();
    expect(handleClusterKey(ev("t", { metaKey: true }), a)).toBe(false);
    expect(handleClusterKey(ev("q"), a)).toBe(false);
  });
  it("ctrl+d deletes with selection", () => {
    const a = actions();
    const e = ev("d", { ctrlKey: true });
    expect(handleClusterKey(e, a)).toBe(true);
    expect(a.del).toHaveBeenCalled();
  });
  it("s/r/c act on selection", () => {
    const a = actions();
    handleClusterKey(ev("s"), a); expect(a.scale).toHaveBeenCalled();
    handleClusterKey(ev("r"), a); expect(a.restart).toHaveBeenCalled();
    handleClusterKey(ev("c"), a); expect(a.cordon).toHaveBeenCalled();
  });
  it("s/r/c/ctrl+d ignored with no selection", () => {
    const a = actions({ hasSelection: () => false });
    expect(handleClusterKey(ev("s"), a)).toBe(false);
    expect(handleClusterKey(ev("d", { ctrlKey: true }), a)).toBe(false);
  });
  it("x opens shell, f opens port-forward", () => {
    const a = actions();
    handleClusterKey(ev("x"), a); expect(a.shell).toHaveBeenCalled();
    handleClusterKey(ev("f"), a); expect(a.forward).toHaveBeenCalled();
  });
  it("x/f ignored with no selection", () => {
    const a = actions({ hasSelection: () => false });
    expect(handleClusterKey(ev("x"), a)).toBe(false);
    expect(handleClusterKey(ev("f"), a)).toBe(false);
    expect(a.shell).not.toHaveBeenCalled();
    expect(a.forward).not.toHaveBeenCalled();
  });
  it("e opens the editable yaml view with selection", () => {
    const a = actions();
    const e = ev("e");
    expect(handleClusterKey(e, a)).toBe(true);
    expect(a.edit).toHaveBeenCalled();
  });
  it("e ignored with no selection", () => {
    const a = actions({ hasSelection: () => false });
    expect(handleClusterKey(ev("e"), a)).toBe(false);
    expect(a.edit).not.toHaveBeenCalled();
  });
  it("u uncordons with selection, ignored without", () => {
    const a = actions();
    expect(handleClusterKey(ev("u"), a)).toBe(true);
    expect(a.uncordon).toHaveBeenCalled();
    const b = actions({ hasSelection: () => false });
    expect(handleClusterKey(ev("u"), b)).toBe(false);
  });
  it("j/k and arrows move the selection; unhandled move falls through", () => {
    const a = actions();
    expect(handleClusterKey(ev("j"), a)).toBe(true);
    expect(a.move).toHaveBeenLastCalledWith(1);
    expect(handleClusterKey(ev("k"), a)).toBe(true);
    expect(a.move).toHaveBeenLastCalledWith(-1);
    expect(handleClusterKey(ev("ArrowDown"), a)).toBe(true);
    expect(a.move).toHaveBeenLastCalledWith(1);
    expect(handleClusterKey(ev("ArrowUp"), a)).toBe(true);
    expect(a.move).toHaveBeenLastCalledWith(-1);
    const b = actions({ move: vi.fn(() => false) });
    const e = ev("j");
    expect(handleClusterKey(e, b)).toBe(false);
    expect(e.preventDefault).not.toHaveBeenCalled();
  });
});

describe("moveSelection", () => {
  const keys = ["a/1", "a/2", "b/1"];
  it("empty list yields null", () => {
    expect(moveSelection([], null, 1)).toBe(null);
    expect(moveSelection([], "a/1", -1)).toBe(null);
  });
  it("no selection starts from the edge in the direction of travel", () => {
    expect(moveSelection(keys, null, 1)).toBe("a/1");
    expect(moveSelection(keys, null, -1)).toBe("b/1");
  });
  it("stale selection (filtered out) restarts from the edge", () => {
    expect(moveSelection(keys, "gone", 1)).toBe("a/1");
    expect(moveSelection(keys, "gone", -1)).toBe("b/1");
  });
  it("steps and clamps at both ends", () => {
    expect(moveSelection(keys, "a/1", 1)).toBe("a/2");
    expect(moveSelection(keys, "a/2", -1)).toBe("a/1");
    expect(moveSelection(keys, "b/1", 1)).toBe("b/1");
    expect(moveSelection(keys, "a/1", -1)).toBe("a/1");
  });
});
