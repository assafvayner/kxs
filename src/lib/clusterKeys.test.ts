import { describe, expect, it, vi } from "vitest";
import { handleClusterKey, type ClusterActions } from "./clusterKeys";

function actions(overrides: Partial<ClusterActions> = {}): ClusterActions {
  return {
    openCommand: vi.fn(),
    openFilter: vi.fn(),
    back: vi.fn(),
    describe: vi.fn(),
    yaml: vi.fn(),
    logs: vi.fn(),
    enter: vi.fn(),
    hasSelection: () => true,
    ...overrides,
  };
}
const ev = (key: string, mods: any = {}) => ({ key, ctrlKey: false, metaKey: false, shiftKey: false, preventDefault: vi.fn(), ...mods });

describe("handleClusterKey", () => {
  it(": opens command, / opens filter", () => {
    const a = actions();
    const e1 = ev(":"); expect(handleClusterKey(e1, a)).toBe(true); expect(a.openCommand).toHaveBeenCalled();
    const e2 = ev("/"); expect(handleClusterKey(e2, a)).toBe(true); expect(a.openFilter).toHaveBeenCalled();
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
    expect(handleClusterKey(ev("x"), a)).toBe(false);
  });
});
