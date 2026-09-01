import type { KeyInput } from "./keys";

export type VimMode = "normal" | "insert" | "ex" | "search";

export type VimOp = "d" | "c" | "y" | ">" | "<";

export type FindCmd = "f" | "F" | "t" | "T";

export interface VimState {
  mode: VimMode;
  op: VimOp | null;
  pending: string;
  exBuf: string;
  searchBuf: string;
  lastSearch: string;
  searchBackward: boolean;
  lastFind: { cmd: FindCmd; ch: string } | null;
  register: string;
  regLinewise: boolean;
  undoStack: Array<{ text: string; caret: number }>;
}

export type VimEffect = "apply" | "close" | "applyClose";

export interface VimResult {
  text: string;
  caret: number;
  state: VimState;
  handled: boolean;
  effect?: VimEffect;
}

const UNDO_CAP = 200;

export function initialVimState(): VimState {
  return {
    mode: "normal",
    op: null,
    pending: "",
    exBuf: "",
    searchBuf: "",
    lastSearch: "",
    searchBackward: false,
    lastFind: null,
    register: "",
    regLinewise: false,
    undoStack: [],
  };
}

// --- offset/line helpers ---------------------------------------------------

function lineStartOf(text: string, pos: number): number {
  return text.lastIndexOf("\n", pos - 1) + 1;
}
function lineEndOf(text: string, pos: number): number {
  const nl = text.indexOf("\n", pos);
  return nl === -1 ? text.length : nl;
}
function colOf(text: string, pos: number): number {
  return pos - lineStartOf(text, pos);
}
function firstNonBlankOf(text: string, ls: number, le: number): number {
  let i = ls;
  while (i < le && /\s/.test(text[i])) i++;
  return i;
}

// 1-based inclusive line range -> {start of first line, end of last line (excl. trailing \n)}
function lineNumberRange(text: string, m: number, n: number): { start: number; end: number } {
  const lines = text.split("\n");
  const lo = Math.min(Math.max(Math.min(m, n), 1), lines.length);
  const hi = Math.min(Math.max(Math.max(m, n), 1), lines.length);
  let start = 0;
  for (let i = 0; i < lo - 1; i++) start += lines[i].length + 1;
  let end = start;
  for (let i = lo - 1; i < hi; i++) end += lines[i].length + (i > lo - 1 ? 1 : 0);
  // 'end' now points at the end of the hi-th line's content
  return { start, end };
}

function charClass(c: string, big = false): 0 | 1 | 2 {
  if (/\s/.test(c)) return 0;
  if (big) return 1;
  if (/\w/.test(c)) return 1;
  return 2;
}
function wordForward(text: string, caret: number, big = false): number {
  const n = text.length;
  let i = caret;
  if (i >= n) return n;
  const cls = charClass(text[i], big);
  if (cls !== 0) while (i < n && charClass(text[i], big) === cls) i++;
  while (i < n && charClass(text[i], big) === 0) i++;
  return i;
}
function wordBackward(text: string, caret: number, big = false): number {
  let i = caret - 1;
  while (i >= 0 && charClass(text[i], big) === 0) i--;
  if (i < 0) return 0;
  const cls = charClass(text[i], big);
  while (i >= 0 && charClass(text[i], big) === cls) i--;
  return i + 1;
}
// Last character of the current or next word (inclusive motion, like vim's e/E).
function wordEnd(text: string, caret: number, big = false): number {
  const n = text.length;
  let i = caret + 1;
  while (i < n && charClass(text[i], big) === 0) i++;
  if (i >= n) return Math.max(caret, n - 1);
  const cls = charClass(text[i], big);
  while (i + 1 < n && charClass(text[i + 1], big) === cls) i++;
  return i;
}

// Target of f/F/t/T within the current line; returns `caret` when the search fails.
// `repeat` skips an adjacent target so ;/, do not stand still on t/T.
function findChar(text: string, caret: number, cmd: FindCmd, ch: string, repeat = false): number {
  if (cmd === "f" || cmd === "t") {
    const le = lineEndOf(text, caret);
    for (let i = caret + (cmd === "t" && repeat ? 2 : 1); i < le; i++) {
      if (text[i] !== ch) continue;
      const target = cmd === "f" ? i : i - 1;
      return target > caret ? target : caret;
    }
    return caret;
  }
  const ls = lineStartOf(text, caret);
  for (let i = caret - (cmd === "T" && repeat ? 2 : 1); i >= ls; i--) {
    if (text[i] !== ch) continue;
    const target = cmd === "F" ? i : i + 1;
    return target < caret ? target : caret;
  }
  return caret;
}
function reverseFind(cmd: FindCmd): FindCmd {
  return cmd === "f" ? "F" : cmd === "F" ? "f" : cmd === "t" ? "T" : "t";
}

// Next/previous empty line (vim's } / {), clamped to the buffer ends.
function paragraphForward(text: string, caret: number): number {
  let pos = lineEndOf(text, caret);
  while (pos < text.length) {
    const ls = pos + 1;
    const le = lineEndOf(text, ls);
    if (le === ls) return ls;
    pos = le;
  }
  return text.length;
}
function paragraphBackward(text: string, caret: number): number {
  let ls = lineStartOf(text, caret);
  while (ls > 0) {
    const prevStart = lineStartOf(text, ls - 1);
    if (prevStart === ls - 1) return prevStart;
    ls = prevStart;
  }
  return 0;
}

const PAIRS = "()[]{}";
// Match of the first bracket at/after the caret on this line; `caret` when there is none.
function matchPair(text: string, caret: number): number {
  const le = lineEndOf(text, caret);
  let i = caret;
  while (i < le && !PAIRS.includes(text[i])) i++;
  if (i >= le) return caret;
  const k = PAIRS.indexOf(text[i]);
  const forward = k % 2 === 0;
  const open = PAIRS[forward ? k : k - 1];
  const close = PAIRS[forward ? k + 1 : k];
  let depth = 0;
  if (forward) {
    for (let j = i; j < text.length; j++) {
      if (text[j] === open) depth++;
      else if (text[j] === close && --depth === 0) return j;
    }
  } else {
    for (let j = i; j >= 0; j--) {
      if (text[j] === close) depth++;
      else if (text[j] === open && --depth === 0) return j;
    }
  }
  return caret;
}

// The \w word at/after the caret on this line (vim's * / # target).
function wordAt(text: string, caret: number): string {
  const le = lineEndOf(text, caret);
  let i = caret;
  while (i < le && !/\w/.test(text[i])) i++;
  if (i >= le) return "";
  let from = i;
  while (from > 0 && /\w/.test(text[from - 1])) from--;
  let to = i;
  while (to < le && /\w/.test(text[to])) to++;
  return text.slice(from, to);
}

const SHIFT = "  ";
function shiftRegion(text: string, from: number, to: number, dir: 1 | -1): string {
  const lines = text
    .slice(from, to)
    .split("\n")
    .map((l) => {
      if (dir === 1) return l.length === 0 ? l : SHIFT + l;
      return l.replace(/^(\t| {1,2})/, "");
    });
  return text.slice(0, from) + lines.join("\n") + text.slice(to);
}

function snapshot(st: VimState, text: string, caret: number): VimState {
  const undoStack = [...st.undoStack, { text, caret }];
  if (undoStack.length > UNDO_CAP) undoStack.shift();
  return { ...st, undoStack };
}

// --- result builders -------------------------------------------------------

function pass(text: string, caret: number, st: VimState): VimResult {
  return { text, caret, state: st, handled: false };
}
function done(text: string, caret: number, st: VimState, effect?: VimEffect): VimResult {
  return { text, caret, state: st, handled: true, effect };
}

// --- dispatcher ------------------------------------------------------------

export function vimKey(e: KeyInput, text: string, caret: number, st: VimState): VimResult {
  switch (st.mode) {
    case "insert":
      return handleInsert(e, text, caret, st);
    case "ex":
      return handleEx(e, text, caret, st);
    case "search":
      return handleSearch(e, text, caret, st);
    default:
      return handleNormal(e, text, caret, st);
  }
}

function handleInsert(e: KeyInput, text: string, caret: number, st: VimState): VimResult {
  if (e.key === "Escape") return done(text, caret, { ...st, mode: "normal", pending: "" });
  return pass(text, caret, st); // native typing / shortcuts
}

function handleNormal(e: KeyInput, text: string, caret: number, st: VimState): VimResult {
  if (e.metaKey || e.ctrlKey) return pass(text, caret, st);
  if (e.key.length !== 1 && e.key !== "Escape") return pass(text, caret, st);

  if (st.op) return handleOperator(e, text, caret, st);

  // pending "g" (for gg)
  if (st.pending === "g") {
    const cleared = { ...st, pending: "" };
    if (e.key === "g") return done(text, 0, cleared);
    return handleNormal(e, text, caret, cleared);
  }
  if (st.pending) return handlePending(e, text, caret, st);

  const ls = lineStartOf(text, caret);
  const le = lineEndOf(text, caret);
  switch (e.key) {
    case "h":
      return done(text, Math.max(ls, caret - 1), st);
    case "l":
      return done(text, Math.min(le, caret + 1), st);
    case "j":
      return done(text, moveDown(text, caret), st);
    case "k":
      return done(text, moveUp(text, caret), st);
    case "0":
      return done(text, ls, st);
    case "$":
      return done(text, le, st);
    case "G":
      return done(text, lineStartOf(text, text.length), st);
    case "g":
      return done(text, caret, { ...st, pending: "g" });
    case "^":
      return done(text, firstNonBlankOf(text, ls, le), st);
    case "w":
      return done(text, wordForward(text, caret), st);
    case "b":
      return done(text, wordBackward(text, caret), st);
    case "W":
      return done(text, wordForward(text, caret, true), st);
    case "B":
      return done(text, wordBackward(text, caret, true), st);
    case "e":
      return done(text, wordEnd(text, caret), st);
    case "E":
      return done(text, wordEnd(text, caret, true), st);
    case "f":
    case "F":
    case "t":
    case "T":
      return done(text, caret, { ...st, pending: e.key });
    case ";":
    case ",": {
      if (!st.lastFind) return done(text, caret, st);
      const cmd = e.key === ";" ? st.lastFind.cmd : reverseFind(st.lastFind.cmd);
      return done(text, findChar(text, caret, cmd, st.lastFind.ch, true), st);
    }
    case "}":
      return done(text, paragraphForward(text, caret), st);
    case "{":
      return done(text, paragraphBackward(text, caret), st);
    case "%":
      return done(text, matchPair(text, caret), st);
    case "r":
    case "Z":
      return done(text, caret, { ...st, pending: e.key });
    case "i":
      return done(text, caret, snapshot({ ...st, mode: "insert" }, text, caret));
    case "a":
      return done(text, Math.min(le, caret + 1), snapshot({ ...st, mode: "insert" }, text, caret));
    case "I":
      return done(
        text,
        firstNonBlankOf(text, ls, le),
        snapshot({ ...st, mode: "insert" }, text, caret),
      );
    case "A":
      return done(text, le, snapshot({ ...st, mode: "insert" }, text, caret));
    case "o": {
      const st2 = snapshot({ ...st, mode: "insert" }, text, caret);
      return done(text.slice(0, le) + "\n" + text.slice(le), le + 1, st2);
    }
    case "O": {
      const st2 = snapshot({ ...st, mode: "insert" }, text, caret);
      return done(text.slice(0, ls) + "\n" + text.slice(ls), ls, st2);
    }
    case "s": {
      const st2 = snapshot({ ...st, mode: "insert" }, text, caret);
      if (caret >= le) return done(text, caret, st2);
      return done(text.slice(0, caret) + text.slice(caret + 1), caret, st2);
    }
    case "S": {
      const st2 = snapshot({ ...st, mode: "insert" }, text, caret);
      return done(text.slice(0, ls) + text.slice(le), ls, st2);
    }
    case "C": {
      const st2 = snapshot({ ...st, mode: "insert" }, text, caret);
      return done(text.slice(0, caret) + text.slice(le), caret, st2);
    }
    case "Escape":
      return done(text, caret, { ...st, pending: "", op: null });
    case "d":
    case "c":
    case "y":
    case ">":
    case "<":
      return done(text, caret, { ...st, op: e.key as VimOp });
    case ":":
      return done(text, caret, { ...st, mode: "ex", exBuf: "" });
    case "/":
      return done(text, caret, { ...st, mode: "search", searchBuf: "", searchBackward: false });
    case "?":
      return done(text, caret, { ...st, mode: "search", searchBuf: "", searchBackward: true });
    default:
      return editKeys(e, text, caret, st); // edits/undo/search-repeat
  }
}

function moveDown(text: string, caret: number): number {
  const le = lineEndOf(text, caret);
  if (le === text.length) return caret;
  const nextStart = le + 1;
  const nextEnd = lineEndOf(text, nextStart);
  return Math.min(nextStart + colOf(text, caret), nextEnd);
}
function moveUp(text: string, caret: number): number {
  const ls = lineStartOf(text, caret);
  if (ls === 0) return caret;
  const prevStart = lineStartOf(text, ls - 1);
  const prevEnd = ls - 1;
  return Math.min(prevStart + colOf(text, caret), prevEnd);
}

// Next occurrence of needle strictly after `from`, wrapping to the top.
function findNext(text: string, from: number, needle: string): number {
  if (!needle) return from;
  const i = text.indexOf(needle, from + 1);
  if (i !== -1) return i;
  const wrapped = text.indexOf(needle);
  return wrapped === -1 ? from : wrapped;
}
// Previous occurrence of needle strictly before `from`, wrapping to the bottom.
function findPrev(text: string, from: number, needle: string): number {
  if (!needle) return from;
  const i = text.lastIndexOf(needle, Math.max(0, from - 1));
  if (i !== -1 && i < from) return i;
  const wrapped = text.lastIndexOf(needle);
  return wrapped === -1 ? from : wrapped;
}
function searchStep(text: string, caret: number, st: VimState, backward: boolean): number {
  if (!st.lastSearch) return caret;
  return backward
    ? findPrev(text, caret, st.lastSearch)
    : findNext(text, caret, st.lastSearch);
}

// Second key of a two-key normal-mode command (f/F/t/T target, r replacement, Z…).
function handlePending(e: KeyInput, text: string, caret: number, st: VimState): VimResult {
  const cleared = { ...st, pending: "" };
  if (e.key === "Escape" || e.key.length !== 1) return done(text, caret, cleared);
  if (st.pending === "Z") {
    if (e.key === "Z") return done(text, caret, cleared, "applyClose");
    if (e.key === "Q") return done(text, caret, cleared, "close");
    return done(text, caret, cleared);
  }
  if (st.pending === "r") {
    if (caret >= lineEndOf(text, caret)) return done(text, caret, cleared);
    const st2 = snapshot(cleared, text, caret);
    return done(text.slice(0, caret) + e.key + text.slice(caret + 1), caret, st2);
  }
  const cmd = st.pending as FindCmd;
  return done(text, findChar(text, caret, cmd, e.key), {
    ...cleared,
    lastFind: { cmd, ch: e.key },
  });
}

// --- edit/operator/ex/search handlers --------------------------------------

function editKeys(e: KeyInput, text: string, caret: number, st: VimState): VimResult {
  const ls = lineStartOf(text, caret);
  const le = lineEndOf(text, caret);
  switch (e.key) {
    case "x": {
      if (caret >= le) return done(text, caret, st);
      const st2 = snapshot(st, text, caret);
      return done(text.slice(0, caret) + text.slice(caret + 1), caret, {
        ...st2,
        register: text[caret],
        regLinewise: false,
      });
    }
    case "X": {
      if (caret <= ls) return done(text, caret, st);
      const st2 = snapshot(st, text, caret);
      return done(text.slice(0, caret - 1) + text.slice(caret), caret - 1, {
        ...st2,
        register: text[caret - 1],
        regLinewise: false,
      });
    }
    case "D": {
      if (caret >= le) return done(text, caret, st);
      const st2 = snapshot(st, text, caret);
      return done(text.slice(0, caret) + text.slice(le), caret, {
        ...st2,
        register: text.slice(caret, le),
        regLinewise: false,
      });
    }
    case "Y":
      return done(text, caret, { ...st, register: text.slice(ls, le), regLinewise: true });
    case "J": {
      if (le >= text.length) return done(text, caret, st);
      const nextEnd = lineEndOf(text, le + 1);
      const nextStart = firstNonBlankOf(text, le + 1, nextEnd);
      const head = text.slice(0, le);
      const sep = nextStart >= nextEnd || /\s$/.test(head) || text[nextStart] === ")" ? "" : " ";
      const st2 = snapshot(st, text, caret);
      return done(head + sep + text.slice(nextStart), le, st2);
    }
    case "~": {
      if (caret >= le) return done(text, caret, st);
      const c = text[caret];
      const flipped = c === c.toLowerCase() ? c.toUpperCase() : c.toLowerCase();
      const st2 = snapshot(st, text, caret);
      return done(text.slice(0, caret) + flipped + text.slice(caret + 1), caret + 1, st2);
    }
    case "*":
    case "#": {
      const needle = wordAt(text, caret);
      if (!needle) return done(text, caret, st);
      const backward = e.key === "#";
      const next = backward ? findPrev(text, caret, needle) : findNext(text, caret, needle);
      return done(text, next, { ...st, lastSearch: needle, searchBackward: backward });
    }
    case "P": {
      if (!st.register) return done(text, caret, st);
      const st2 = snapshot(st, text, caret);
      if (st.regLinewise) {
        return done(text.slice(0, ls) + st.register + "\n" + text.slice(ls), ls, st2);
      }
      return done(
        text.slice(0, caret) + st.register + text.slice(caret),
        caret + st.register.length,
        st2,
      );
    }
    case "p": {
      if (!st.register) return done(text, caret, st);
      const st2 = snapshot(st, text, caret);
      if (st.regLinewise) {
        const at = lineEndOf(text, caret);
        return done(text.slice(0, at) + "\n" + st.register + text.slice(at), at + 1, st2);
      }
      return done(
        text.slice(0, caret) + st.register + text.slice(caret),
        caret + st.register.length,
        st2,
      );
    }
    case "u": {
      if (st.undoStack.length === 0) return done(text, caret, st);
      const undoStack = [...st.undoStack];
      const snap = undoStack.pop()!;
      return done(snap.text, snap.caret, { ...st, undoStack });
    }
    case "n":
      return done(text, searchStep(text, caret, st, st.searchBackward), st);
    case "N":
      return done(text, searchStep(text, caret, st, !st.searchBackward), st);
    default:
      return done(text, caret, st); // swallow unmapped keys
  }
}
interface Span {
  from: number;
  to: number;
  linewise: boolean;
}

// Charwise/linewise range for an operator motion. Returns null while awaiting
// a second key (operator-pending "g"), which the caller keeps pending.
function operatorSpan(
  e: KeyInput,
  text: string,
  caret: number,
  op: VimOp,
  pending: string,
): Span | null | "await" {
  if (pending) {
    if (pending === "g") {
      if (e.key === "g") return linewiseSpan(text, caret, 0); // dgg
      return null; // abort
    }
    const cmd = pending as FindCmd;
    const target = findChar(text, caret, cmd, e.key);
    if (target === caret) return null;
    return cmd === "f" || cmd === "t"
      ? { from: caret, to: target + 1, linewise: false }
      : { from: target, to: caret, linewise: false };
  }
  switch (e.key) {
    case op:
      return linewiseSpan(text, caret, caret); // dd/cc/yy/>>/<<
    case "g":
    case "f":
    case "F":
    case "t":
    case "T":
      return "await";
    case "G":
      return linewiseSpan(text, caret, text.length);
    case "j": {
      const to = moveDown(text, caret);
      return to === caret ? null : linewiseSpan(text, caret, to);
    }
    case "k": {
      const to = moveUp(text, caret);
      return to === caret ? null : linewiseSpan(text, caret, to);
    }
    case "w":
      return { from: caret, to: wordForward(text, caret), linewise: false };
    case "W":
      return { from: caret, to: wordForward(text, caret, true), linewise: false };
    case "b":
      return { from: wordBackward(text, caret), to: caret, linewise: false };
    case "B":
      return { from: wordBackward(text, caret, true), to: caret, linewise: false };
    case "e":
      return { from: caret, to: wordEnd(text, caret) + 1, linewise: false };
    case "E":
      return { from: caret, to: wordEnd(text, caret, true) + 1, linewise: false };
    case "h":
      return caret <= lineStartOf(text, caret)
        ? null
        : { from: caret - 1, to: caret, linewise: false };
    case "l":
      return caret >= lineEndOf(text, caret)
        ? null
        : { from: caret, to: caret + 1, linewise: false };
    case "0":
      return { from: lineStartOf(text, caret), to: caret, linewise: false };
    case "^": {
      const fnb = firstNonBlankOf(text, lineStartOf(text, caret), lineEndOf(text, caret));
      return fnb >= caret ? null : { from: fnb, to: caret, linewise: false };
    }
    case "$":
      return { from: caret, to: lineEndOf(text, caret), linewise: false };
    case "}":
      return { from: caret, to: paragraphForward(text, caret), linewise: false };
    case "{":
      return { from: paragraphBackward(text, caret), to: caret, linewise: false };
    case "%": {
      const m = matchPair(text, caret);
      if (m === caret) return null;
      return m > caret
        ? { from: caret, to: m + 1, linewise: false }
        : { from: m, to: caret + 1, linewise: false };
    }
    default:
      return null; // unknown motion => abort operator
  }
}

// Linewise span covering the lines touched by carets a..b.
function linewiseSpan(text: string, a: number, b: number): Span {
  const lo = Math.min(a, b);
  const hi = Math.max(a, b);
  return { from: lineStartOf(text, lo), to: lineEndOf(text, hi), linewise: true };
}

function handleOperator(e: KeyInput, text: string, caret: number, st: VimState): VimResult {
  const op = st.op!;
  if (e.key === "Escape") return done(text, caret, { ...st, op: null, pending: "" });

  const span = operatorSpan(e, text, caret, op, st.pending);
  if (span === "await") return done(text, caret, { ...st, pending: e.key });
  if (span === null) return done(text, caret, { ...st, op: null, pending: "" });

  const { from, to, linewise } = span;
  const cleared = { ...st, op: null, pending: "" };

  if (op === ">" || op === "<") {
    // indent operators are always linewise over the lines the motion touched
    const lw = linewiseSpan(text, from, to);
    const st2 = snapshot(cleared, text, caret);
    const newText = shiftRegion(text, lw.from, lw.to, op === ">" ? 1 : -1);
    return done(
      newText,
      firstNonBlankOf(newText, lw.from, lineEndOf(newText, lw.from)),
      st2,
    );
  }

  if (op === "y") {
    const register = text.slice(from, to);
    return done(text, from, { ...cleared, register, regLinewise: linewise });
  }

  // d / c: remove the span (linewise also removes one surrounding newline)
  const st2 = snapshot(cleared, text, caret);
  const register = text.slice(from, to);
  let cutFrom = from;
  let cutTo = to;
  if (linewise) {
    if (to < text.length) cutTo = to + 1; // consume trailing newline
    else if (from > 0) cutFrom = from - 1; // last line: consume leading newline
  }
  const newText = text.slice(0, cutFrom) + text.slice(cutTo);
  const newCaret = Math.min(cutFrom, newText.length);
  const base = { ...st2, register, regLinewise: linewise };
  if (op === "c") {
    if (linewise) {
      // cc keeps an empty line to type into
      const reopened = text.slice(0, from) + text.slice(to);
      return done(reopened, from, { ...base, mode: "insert", regLinewise: linewise });
    }
    return done(newText, newCaret, { ...base, mode: "insert" });
  }
  return done(newText, newCaret, base);
}
function handleEx(e: KeyInput, text: string, caret: number, st: VimState): VimResult {
  if (e.key === "Escape") return done(text, caret, { ...st, mode: "normal", exBuf: "" });
  if (e.key === "Backspace") {
    return done(text, caret, { ...st, exBuf: st.exBuf.slice(0, -1) });
  }
  if (e.key === "Enter") return execEx(text, caret, st);
  if (e.key.length === 1) return done(text, caret, { ...st, exBuf: st.exBuf + e.key });
  return pass(text, caret, st);
}

function execEx(text: string, caret: number, st: VimState): VimResult {
  const buf = st.exBuf.trim();
  const normal = { ...st, mode: "normal" as VimMode, exBuf: "" };

  if (buf === "w") return done(text, caret, normal, "apply");
  if (buf === "q") return done(text, caret, normal, "close");
  if (buf === "wq") return done(text, caret, normal, "applyClose");

  const m = buf.match(/^(\d+)(?:,(\d+))?([dy])?$/);
  if (!m) return done(text, caret, normal);

  const a = Number(m[1]);
  const b = m[2] ? Number(m[2]) : a;
  const action = m[3];

  if (!action) {
    // :N -> jump to the first non-blank char of line N (clamped to the last line)
    const { start, end } = lineNumberRange(text, a, a);
    return done(text, firstNonBlankOf(text, start, end), normal);
  }

  const { start, end } = lineNumberRange(text, a, b);
  const register = text.slice(start, end);

  if (action === "y") {
    return done(text, start, { ...normal, register, regLinewise: true });
  }

  // action === "d": delete the lines incl. one surrounding newline
  const st2 = snapshot(normal, text, caret);
  let cutFrom = start;
  let cutTo = end;
  if (end < text.length) cutTo = end + 1;
  else if (start > 0) cutFrom = start - 1;
  const newText = text.slice(0, cutFrom) + text.slice(cutTo);
  return done(newText, Math.min(cutFrom, newText.length), {
    ...st2,
    register,
    regLinewise: true,
  });
}
function handleSearch(e: KeyInput, text: string, caret: number, st: VimState): VimResult {
  if (e.key === "Escape") return done(text, caret, { ...st, mode: "normal", searchBuf: "" });
  if (e.key === "Backspace") {
    return done(text, caret, { ...st, searchBuf: st.searchBuf.slice(0, -1) });
  }
  if (e.key === "Enter") {
    const needle = st.searchBuf;
    const next = st.searchBackward
      ? findPrev(text, caret, needle)
      : findNext(text, caret, needle);
    return done(text, next, { ...st, mode: "normal", searchBuf: "", lastSearch: needle });
  }
  if (e.key.length === 1) return done(text, caret, { ...st, searchBuf: st.searchBuf + e.key });
  return pass(text, caret, st);
}
