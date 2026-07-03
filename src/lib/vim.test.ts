import { describe, expect, it, vi } from "vitest";
import { initialVimState, vimKey, type VimMode, type VimState } from "./vim";

const ev = (
  key: string,
  mods: Partial<{ ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }> = {},
) => ({ key, ctrlKey: false, metaKey: false, shiftKey: false, preventDefault: vi.fn(), ...mods });

/** Feed a sequence of keys; return final {text, caret, mode, effect}. */
function run(text: string, caret: number, keys: string[], start?: Partial<VimState>) {
  let st: VimState = { ...initialVimState(), ...start };
  let effect: string | undefined;
  for (const k of keys) {
    const r = vimKey(ev(k), text, caret, st);
    st = r.state;
    if (r.handled) {
      text = r.text;
      caret = r.caret;
    }
    if (r.effect) effect = r.effect;
  }
  return { text, caret, mode: st.mode as VimMode, effect };
}

describe("vim motions", () => {
  const T = "abc\ndef\nghi";
  it("h/l move within a line and clamp", () => {
    expect(run(T, 1, ["h"]).caret).toBe(0);
    expect(run(T, 0, ["h"]).caret).toBe(0);
    expect(run(T, 1, ["l"]).caret).toBe(2);
    expect(run(T, 3, ["l"]).caret).toBe(3); // clamp at line end (before \n)
  });
  it("j/k move vertically preserving column", () => {
    expect(run(T, 1, ["j"]).caret).toBe(5); // 'b' -> 'e'
    expect(run(T, 5, ["k"]).caret).toBe(1);
    expect(run(T, 1, ["k"]).caret).toBe(1); // first line, no move
  });
  it("0 and $ jump to line ends", () => {
    expect(run(T, 5, ["0"]).caret).toBe(4);
    expect(run(T, 5, ["$"]).caret).toBe(7);
  });
  it("gg goes to top, G to start of last line", () => {
    expect(run(T, 5, ["g", "g"]).caret).toBe(0);
    expect(run(T, 0, ["G"]).caret).toBe(8);
  });
  it("w/b move by word", () => {
    const W = "foo bar baz";
    expect(run(W, 0, ["w"]).caret).toBe(4);
    expect(run(W, 4, ["b"]).caret).toBe(0);
  });
});

describe("vim insert entry", () => {
  it("i enters insert mode, caret unchanged", () => {
    const r = run("abc", 1, ["i"]);
    expect(r.mode).toBe("insert");
    expect(r.caret).toBe(1);
  });
  it("a enters insert after the caret", () => {
    const r = run("abc", 1, ["a"]);
    expect(r.mode).toBe("insert");
    expect(r.caret).toBe(2);
  });
  it("o opens a line below and enters insert", () => {
    const r = run("abc\ndef", 1, ["o"]);
    expect(r.text).toBe("abc\n\ndef");
    expect(r.caret).toBe(4);
    expect(r.mode).toBe("insert");
  });
  it("O opens a line above and enters insert", () => {
    const r = run("abc\ndef", 5, ["O"]);
    expect(r.text).toBe("abc\n\ndef");
    expect(r.caret).toBe(4);
    expect(r.mode).toBe("insert");
  });
  it("Escape returns to normal mode", () => {
    const r = run("abc", 1, ["i", "Escape"]);
    expect(r.mode).toBe("normal");
  });
});

describe("vim passthrough", () => {
  it("does not handle modifier combos in normal mode", () => {
    const r = vimKey(ev("t", { metaKey: true }), "abc", 0, initialVimState());
    expect(r.handled).toBe(false);
  });
  it("does not handle printable keys in insert mode (native typing)", () => {
    const st = { ...initialVimState(), mode: "insert" as const };
    const r = vimKey(ev("x"), "abc", 1, st);
    expect(r.handled).toBe(false);
  });
});
