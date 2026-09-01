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
  it("^ jumps to the first non-blank of the line", () => {
    expect(run("  abc\ndef", 4, ["^"]).caret).toBe(2);
    expect(run("abc", 2, ["^"]).caret).toBe(0);
  });
  it("e moves to the end of the current/next word", () => {
    const W = "foo bar baz";
    expect(run(W, 0, ["e"]).caret).toBe(2);
    expect(run(W, 2, ["e"]).caret).toBe(6);
    expect(run(W, 10, ["e"]).caret).toBe(10); // last word already at its end
  });
  it("W/B/E treat punctuation as part of the WORD", () => {
    const W = "a.b cd";
    expect(run(W, 0, ["w"]).caret).toBe(1); // small word stops at '.'
    expect(run(W, 0, ["W"]).caret).toBe(4);
    expect(run(W, 0, ["E"]).caret).toBe(2);
    expect(run(W, 4, ["B"]).caret).toBe(0);
  });
  it("f/F move to a char on the line, t/T stop next to it", () => {
    const W = "foo bar baz";
    expect(run(W, 0, ["f", "b"]).caret).toBe(4);
    expect(run(W, 0, ["t", "b"]).caret).toBe(3);
    expect(run(W, 10, ["F", "b"]).caret).toBe(8);
    expect(run(W, 10, ["T", "b"]).caret).toBe(9);
  });
  it("f does not cross a line boundary", () => {
    expect(run("abc\nxbz", 0, ["f", "z"]).caret).toBe(0);
  });
  it("; repeats and , reverses the last f/t", () => {
    const W = "a-b-c-d";
    expect(run(W, 0, ["f", "-"]).caret).toBe(1);
    expect(run(W, 0, ["f", "-", ";"]).caret).toBe(3);
    expect(run(W, 0, ["f", "-", ";", ","]).caret).toBe(1);
    expect(run(W, 0, ["t", "-", ";"]).caret).toBe(2); // ; does not stand still on t
  });
  it("; is a no-op without a prior f/t", () => {
    expect(run("a-b", 0, [";"]).caret).toBe(0);
  });
  it("}/{ move by paragraph", () => {
    const P = "a\nb\n\nc\nd";
    expect(run(P, 0, ["}"]).caret).toBe(4);
    expect(run(P, 6, ["{"]).caret).toBe(4);
    expect(run(P, 6, ["}"]).caret).toBe(P.length); // no further boundary
    expect(run(P, 0, ["{"]).caret).toBe(0);
  });
  it("% jumps between matching brackets", () => {
    const B = "a (b [c] d) e";
    expect(run(B, 0, ["%"]).caret).toBe(10);
    expect(run(B, 10, ["%"]).caret).toBe(2);
    expect(run(B, 5, ["%"]).caret).toBe(7);
    expect(run("abc", 0, ["%"]).caret).toBe(0); // no bracket on the line
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
  it("I enters insert at the first non-blank of the line", () => {
    const r = run("  abc\ndef", 4, ["I"]);
    expect(r.mode).toBe("insert");
    expect(r.caret).toBe(2);
  });
  it("I on a line with no leading blanks behaves like caret unchanged if already there", () => {
    const r = run("abc\ndef", 5, ["I"]);
    expect(r.mode).toBe("insert");
    expect(r.caret).toBe(4);
  });
  it("A enters insert at the end of the line", () => {
    const r = run("abc\ndef", 0, ["A"]);
    expect(r.mode).toBe("insert");
    expect(r.caret).toBe(3);
    expect(r.text).toBe("abc\ndef");
  });
  it("s deletes the char under the caret and enters insert", () => {
    const r = run("abc", 1, ["s"]);
    expect(r.text).toBe("ac");
    expect(r.caret).toBe(1);
    expect(r.mode).toBe("insert");
  });
  it("s at end of line does not delete but still enters insert", () => {
    const r = run("abc", 3, ["s"]);
    expect(r.text).toBe("abc");
    expect(r.caret).toBe(3);
    expect(r.mode).toBe("insert");
  });
  it("S clears the current line and enters insert", () => {
    const r = run("abc\ndef", 5, ["S"]);
    expect(r.text).toBe("abc\n");
    expect(r.caret).toBe(4);
    expect(r.mode).toBe("insert");
  });
  it("C deletes from the caret to end of line and enters insert", () => {
    const r = run("abc\ndef", 1, ["C"]);
    expect(r.text).toBe("a\ndef");
    expect(r.caret).toBe(1);
    expect(r.mode).toBe("insert");
  });
  it("I, A, s, S, C each take an undo snapshot", () => {
    expect(run("  abc", 4, ["I", "Escape", "u"]).text).toBe("  abc");
    expect(run("abc", 0, ["A", "Escape", "u"]).text).toBe("abc");
    expect(run("abc", 1, ["s", "Escape", "u"]).text).toBe("abc");
    expect(run("abc\ndef", 5, ["S", "Escape", "u"]).text).toBe("abc\ndef");
    expect(run("abc", 1, ["C", "Escape", "u"]).text).toBe("abc");
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
  it("P pastes charwise text before the caret", () => {
    const r = run("abc", 0, ["x", "l", "P"]);
    // 'a' deleted -> "bc" caret 0; 'l' -> caret 1; P inserts 'a' at caret 1 -> "bac"
    expect(r.text).toBe("bac");
  });
  it("P pastes a linewise register above the current line", () => {
    const r = run("l1\nl2", 3, ["Y", "P"]);
    expect(r.text).toBe("l1\nl2\nl2");
    expect(r.caret).toBe(3);
  });
  it("P with an empty register is a no-op", () => {
    expect(run("abc", 0, ["P"]).text).toBe("abc");
  });
});

describe("vim normal-mode edits", () => {
  it("X deletes the char before the caret", () => {
    const r = run("abc", 2, ["X"]);
    expect(r.text).toBe("ac");
    expect(r.caret).toBe(1);
  });
  it("X at the line start is a no-op", () => {
    expect(run("abc\ndef", 4, ["X"]).text).toBe("abc\ndef");
  });
  it("X then p pastes the deleted char", () => {
    const r = run("abc", 1, ["X", "l", "p"]);
    // 'a' deleted -> "bc" caret 0; 'l' -> caret 1; p inserts 'a' -> "bac"
    expect(r.text).toBe("bac");
  });
  it("D deletes to the end of the line", () => {
    const r = run("abc\ndef", 1, ["D"]);
    expect(r.text).toBe("a\ndef");
    expect(r.caret).toBe(1);
  });
  it("D at the end of the line is a no-op", () => {
    expect(run("abc", 3, ["D"]).text).toBe("abc");
  });
  it("D then p pastes the deleted tail", () => {
    expect(run("abc", 1, ["D", "p"]).text).toBe("abc");
  });
  it("Y yanks the line so p pastes it below", () => {
    const r = run("l1\nl2", 0, ["Y", "p"]);
    expect(r.text).toBe("l1\nl1\nl2");
  });
  it("J joins the next line with a single space", () => {
    const r = run("foo\n  bar", 1, ["J"]);
    expect(r.text).toBe("foo bar");
    expect(r.caret).toBe(3);
  });
  it("J adds no space when the line already ends in one", () => {
    expect(run("foo \nbar", 0, ["J"]).text).toBe("foo bar");
  });
  it("J onto a blank line just removes the newline", () => {
    expect(run("foo\n\nbar", 0, ["J"]).text).toBe("foo\nbar");
  });
  it("J on the last line is a no-op", () => {
    expect(run("foo", 0, ["J"]).text).toBe("foo");
  });
  it("r replaces the char under the caret", () => {
    const r = run("abc", 1, ["r", "X"]);
    expect(r.text).toBe("aXc");
    expect(r.caret).toBe(1);
  });
  it("r with Escape cancels without editing", () => {
    expect(run("abc", 1, ["r", "Escape"]).text).toBe("abc");
  });
  it("r at the end of the line is a no-op", () => {
    expect(run("abc\ndef", 3, ["r", "X"]).text).toBe("abc\ndef");
  });
  it("~ toggles case and advances", () => {
    const r = run("abc", 0, ["~", "~"]);
    expect(r.text).toBe("ABc");
    expect(r.caret).toBe(2);
  });
  it("~ at the end of the line is a no-op", () => {
    expect(run("abc", 3, ["~"]).text).toBe("abc");
  });
  it("X, D, J, r and ~ each take an undo snapshot", () => {
    expect(run("abc", 2, ["X", "u"]).text).toBe("abc");
    expect(run("abc", 1, ["D", "u"]).text).toBe("abc");
    expect(run("foo\nbar", 0, ["J", "u"]).text).toBe("foo\nbar");
    expect(run("abc", 1, ["r", "X", "u"]).text).toBe("abc");
    expect(run("abc", 0, ["~", "u"]).text).toBe("abc");
  });
  it("Y does not push an undo snapshot", () => {
    expect(run("l1\nl2", 0, ["Y", "u"]).text).toBe("l1\nl2");
  });
  it("ZZ applies and closes, ZQ closes", () => {
    expect(run("abc", 0, ["Z", "Z"]).effect).toBe("applyClose");
    expect(run("abc", 0, ["Z", "Q"]).effect).toBe("close");
    expect(run("abc", 0, ["Z", "x"]).effect).toBeUndefined();
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
  it("c$ deletes to end of line and enters insert", () => {
    const r = run(T, 4, ["c", "$"]);
    expect(r.text).toBe("foo \nbaz qux");
    expect(r.mode).toBe("insert");
  });
  it("c0 deletes to line start and enters insert", () => {
    const r = run(T, 4, ["c", "0"]);
    expect(r.text).toBe("bar\nbaz qux");
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
  it("de deletes through the end of the word", () => {
    const r = run("foo bar", 0, ["d", "e"]);
    expect(r.text).toBe(" bar");
  });
  it("dW/dB use WORD boundaries", () => {
    expect(run("a.b cd", 0, ["d", "W"]).text).toBe("cd");
    expect(run("a.b cd", 4, ["d", "B"]).text).toBe("cd");
  });
  it("d^ deletes back to the first non-blank", () => {
    const r = run("  foo bar", 6, ["d", "^"]);
    expect(r.text).toBe("  bar");
    expect(r.caret).toBe(2);
  });
  it("dfX deletes through the target char, dtX stops before it", () => {
    expect(run("foo bar", 0, ["d", "f", "b"]).text).toBe("ar");
    expect(run("foo bar", 0, ["d", "t", "b"]).text).toBe("bar");
  });
  it("dFX / dTX delete backwards", () => {
    expect(run("foo bar", 6, ["d", "F", " "]).text).toBe("foor");
    expect(run("foo bar", 6, ["d", "T", " "]).text).toBe("foo r");
  });
  it("a failed f target aborts the operator", () => {
    const r = run("foo bar", 0, ["d", "f", "z", "l"]);
    expect(r.text).toBe("foo bar");
    expect(r.caret).toBe(1);
  });
  it("Escape aborts an operator waiting for an f target", () => {
    expect(run("foo bar", 0, ["d", "f", "Escape"]).text).toBe("foo bar");
  });
  it("dj/dk delete two lines linewise", () => {
    expect(run("a\nb\nc", 0, ["d", "j"]).text).toBe("c");
    expect(run("a\nb\nc", 4, ["d", "k"]).text).toBe("a");
    expect(run("a\nb", 2, ["d", "j"]).text).toBe("a\nb"); // no line below
  });
  it("dl deletes forward one char, dh backward one", () => {
    expect(run("abc", 1, ["d", "l"]).text).toBe("ac");
    expect(run("abc", 1, ["d", "h"]).text).toBe("bc");
    expect(run("abc", 0, ["d", "h"]).text).toBe("abc");
  });
  it("d} deletes to the paragraph boundary", () => {
    expect(run("a\nb\n\nc", 0, ["d", "}"]).text).toBe("\nc");
  });
  it("d% deletes a bracket pair", () => {
    expect(run("a (b c) d", 2, ["d", "%"]).text).toBe("a  d");
    expect(run("a (b c) d", 6, ["d", "%"]).text).toBe("a  d");
  });
  it("y with the new motions fills the register", () => {
    expect(run("foo bar", 0, ["y", "e", "$", "p"]).text).toBe("foo barfoo");
    expect(run("foo bar", 0, ["y", "f", "b", "$", "p"]).text).toBe("foo barfoo b");
  });
  it("c reuses the new motions and enters insert", () => {
    const r = run("foo bar", 0, ["c", "e"]);
    expect(r.text).toBe(" bar");
    expect(r.mode).toBe("insert");
  });
  it(">> indents the current line and << dedents it", () => {
    const r = run("a\nb", 0, [">", ">"]);
    expect(r.text).toBe("  a\nb");
    expect(r.caret).toBe(2);
    expect(run("  a\nb", 0, ["<", "<"]).text).toBe("a\nb");
    expect(run(" a\nb", 0, ["<", "<"]).text).toBe("a\nb");
    expect(run("a\nb", 0, ["<", "<"]).text).toBe("a\nb");
  });
  it(">j indents both lines and leaves blank lines alone", () => {
    expect(run("a\nb\nc", 0, [">", "j"]).text).toBe("  a\n  b\nc");
    expect(run("a\n\nb", 0, [">", "G"]).text).toBe("  a\n\n  b");
  });
  it("> with a charwise motion still shifts whole lines", () => {
    expect(run("foo bar", 0, [">", "w"]).text).toBe("  foo bar");
  });
  it("indent is undoable", () => {
    expect(run("a\nb", 0, [">", ">", "u"]).text).toBe("a\nb");
    expect(run("  a\nb", 0, ["<", "<", "u"]).text).toBe("  a\nb");
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

describe("vim ex commands", () => {
  const T = "l1\nl2\nl3\nl4";
  it(": then N jumps to that line", () => {
    const r = run(T, 0, [":", "3", "Enter"]);
    expect(r.caret).toBe(6); // start of "l3"
    expect(r.mode).toBe("normal");
  });
  it(":Nd deletes a single line", () => {
    const r = run(T, 0, [":", "2", "d", "Enter"]);
    expect(r.text).toBe("l1\nl3\nl4");
  });
  it(":M,Nd deletes a line range", () => {
    const r = run(T, 0, [":", "2", ",", "3", "d", "Enter"]);
    expect(r.text).toBe("l1\nl4");
  });
  it(":Ny yanks a line so p pastes it", () => {
    const r = run(T, 0, [":", "2", "y", "Enter", "p"]);
    expect(r.text).toBe("l1\nl2\nl2\nl3\nl4");
  });
  it(":w emits the apply effect", () => {
    const r = run(T, 0, [":", "w", "Enter"]);
    expect(r.effect).toBe("apply");
    expect(r.mode).toBe("normal");
  });
  it("Escape cancels ex mode", () => {
    const r = run(T, 0, [":", "9", "Escape"]);
    expect(r.mode).toBe("normal");
    expect(r.text).toBe(T);
  });
  it(":q emits the close effect", () => {
    const r = run("l1\nl2", 0, [":", "q", "Enter"]);
    expect(r.effect).toBe("close");
    expect(r.mode).toBe("normal");
  });
  it(":wq emits the applyClose effect", () => {
    const r = run("l1\nl2", 0, [":", "w", "q", "Enter"]);
    expect(r.effect).toBe("applyClose");
    expect(r.mode).toBe("normal");
  });
});

describe("vim search", () => {
  const T = "foo bar foo baz foo";
  it("/pattern jumps to the next match after the caret", () => {
    const r = run(T, 0, ["/", "f", "o", "o", "Enter"]);
    expect(r.caret).toBe(8); // second "foo"
    expect(r.mode).toBe("normal");
  });
  it("n repeats the last search", () => {
    const r = run(T, 0, ["/", "f", "o", "o", "Enter", "n"]);
    expect(r.caret).toBe(16); // third "foo"
  });
  it("search wraps to the top", () => {
    const r = run(T, 17, ["/", "f", "o", "o", "Enter"]);
    expect(r.caret).toBe(0);
  });
  it("Escape cancels search mode", () => {
    const r = run(T, 0, ["/", "x", "Escape"]);
    expect(r.mode).toBe("normal");
    expect(r.caret).toBe(0);
  });
  it("N repeats the last search in the opposite direction", () => {
    const r = run(T, 0, ["/", "f", "o", "o", "Enter", "n", "N"]);
    expect(r.caret).toBe(8);
  });
  it("?pattern searches backwards and n keeps that direction", () => {
    const r = run(T, 18, ["?", "f", "o", "o", "Enter"]);
    expect(r.caret).toBe(16);
    expect(run(T, 18, ["?", "f", "o", "o", "Enter", "n"]).caret).toBe(8);
    expect(run(T, 18, ["?", "f", "o", "o", "Enter", "N"]).caret).toBe(0); // wraps forward
  });
  it("backward search wraps to the bottom", () => {
    expect(run(T, 0, ["?", "f", "o", "o", "Enter"]).caret).toBe(16);
  });
  it("n/N without a previous search are no-ops", () => {
    expect(run(T, 4, ["n"]).caret).toBe(4);
    expect(run(T, 4, ["N"]).caret).toBe(4);
  });
  it("* searches forward for the word at the caret, # backward", () => {
    expect(run(T, 8, ["*"]).caret).toBe(16);
    expect(run(T, 8, ["#"]).caret).toBe(0);
    expect(run(T, 9, ["*"]).caret).toBe(16); // caret inside the word
  });
  it("* sets the search so n repeats it", () => {
    expect(run(T, 0, ["*", "n"]).caret).toBe(16);
  });
  it("* with no word on the line is a no-op", () => {
    expect(run("-- ---", 0, ["*"]).caret).toBe(0);
  });
});
