import { describe, expect, it, vi } from "vitest";
import type { EditorView } from "@codemirror/view";
import { exTarget, runEx, setExCommands } from "./vimEx";

function fakeView(): EditorView {
  return {} as EditorView;
}

describe("exTarget", () => {
  it("maps ex command names to command keys", () => {
    expect(exTarget("write")).toBe("write");
    expect(exTarget("quit")).toBe("quit");
    expect(exTarget("wq")).toBe("writeQuit");
    expect(exTarget("x")).toBe("writeQuit");
  });

  it("returns null for unknown names", () => {
    expect(exTarget("sort")).toBeNull();
  });
});

describe("runEx", () => {
  it("invokes the registered callback for the view", () => {
    const view = fakeView();
    const write = vi.fn();
    const writeQuit = vi.fn();
    setExCommands(view, { write, writeQuit });
    expect(runEx(view, "write")).toBe(true);
    expect(runEx(view, "x")).toBe(true);
    expect(write).toHaveBeenCalledOnce();
    expect(writeQuit).toHaveBeenCalledOnce();
  });

  it("is a no-op for a view without commands or a missing callback", () => {
    const view = fakeView();
    expect(runEx(view, "write")).toBe(false);
    setExCommands(view, { write: vi.fn() });
    expect(runEx(view, "quit")).toBe(false);
  });

  it("forgets commands when cleared", () => {
    const view = fakeView();
    const quit = vi.fn();
    setExCommands(view, { quit });
    setExCommands(view, null);
    expect(runEx(view, "quit")).toBe(false);
    expect(quit).not.toHaveBeenCalled();
  });
});
