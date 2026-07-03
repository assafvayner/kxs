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

describe("vim x and p", () => {
  it("x deletes the char at the caret", () => {
    const r = run("abc", 1, ["x"]);
    expect(r.text).toBe("ac");
    expect(r.caret).toBe(1);
  });
  it("x at end of line is a no-op", () => {
    const r = run("abc", 3, ["x"]);
    expect(r.text).toBe("abc");
  });
  it("x then p pastes the deleted char at the caret", () => {
    const r = run("abc", 0, ["x", "l", "p"]);
    // 'a' deleted -> "bc" caret 0; 'l' -> caret 1; p inserts 'a' at caret 1 -> "bac"
    expect(r.text).toBe("bac");
    expect(r.caret).toBe(2);
  });
  it("p with an empty register is a no-op", () => {
    const r = run("abc", 0, ["p"]);
    expect(r.text).toBe("abc");
  });
});

describe("vim operators", () => {
  const T = "foo bar\nbaz qux";
  it("dw deletes to next word", () => {
    const r = run(T, 0, ["d", "w"]);
    expect(r.text).toBe("bar\nbaz qux");
    expect(r.caret).toBe(0);
  });
  it("d$ deletes to end of line", () => {
    const r = run(T, 4, ["d", "$"]);
    expect(r.text).toBe("foo \nbaz qux");
  });
  it("d0 deletes to line start", () => {
    const r = run(T, 4, ["d", "0"]);
    expect(r.text).toBe("bar\nbaz qux");
    expect(r.caret).toBe(0);
  });
  it("dd deletes the current line", () => {
    const r = run(T, 1, ["d", "d"]);
    expect(r.text).toBe("baz qux");
    expect(r.caret).toBe(0);
  });
  it("dd then p pastes the line below", () => {
    const r = run(T, 1, ["d", "d", "p"]);
    expect(r.text).toBe("baz qux\nfoo bar");
  });
  it("cw deletes the word and enters insert mode", () => {
    const r = run(T, 0, ["c", "w"]);
    expect(r.text).toBe("bar\nbaz qux");
    expect(r.mode).toBe("insert");
  });
  it("cc clears the line and enters insert", () => {
    const r = run(T, 1, ["c", "c"]);
    expect(r.text).toBe("\nbaz qux");
    expect(r.mode).toBe("insert");
  });
  it("yw then p copies a word (pasted at the caret)", () => {
    const r = run(T, 0, ["y", "w", "p"]);
    // yw yanks "foo " and leaves the caret at 0; p inserts it at the caret.
    expect(r.text).toBe("foo foo bar\nbaz qux");
  });
  it("dG deletes to the last line (linewise)", () => {
    const r = run("a\nb\nc", 0, ["d", "G"]);
    expect(r.text).toBe("");
  });
  it("dgg deletes to the first line (linewise)", () => {
    const r = run("a\nb\nc", 4, ["d", "g", "g"]);
    expect(r.text).toBe("");
  });
  it("Escape clears a pending operator", () => {
    const r = run(T, 0, ["d", "Escape", "l"]);
    expect(r.text).toBe(T);
    expect(r.caret).toBe(1); // 'l' still moves after the operator is cancelled
  });
});

describe("vim undo", () => {
  it("u restores the buffer after x", () => {
    const r = run("abc", 0, ["x", "u"]);
    expect(r.text).toBe("abc");
    expect(r.caret).toBe(0);
  });
  it("u restores after dd", () => {
    const r = run("a\nb\nc", 0, ["d", "d", "u"]);
    expect(r.text).toBe("a\nb\nc");
  });
  it("u undoes an entire insert session as one step", () => {
    // enter insert (snapshot of "abc"), then Escape, then u
    const r = run("abc", 1, ["i", "Escape", "u"]);
    expect(r.text).toBe("abc");
  });
  it("u with an empty undo stack is a no-op", () => {
    const r = run("abc", 0, ["u"]);
    expect(r.text).toBe("abc");
  });
});
