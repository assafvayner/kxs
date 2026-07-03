import type { KeyInput } from "./keys";

export type VimMode = "normal" | "insert" | "ex" | "search";

export interface VimState {
  mode: VimMode;
  op: "d" | "c" | "y" | null;
  pending: string;
  exBuf: string;
  searchBuf: string;
  lastSearch: string;
  register: string;
  regLinewise: boolean;
  undoStack: Array<{ text: string; caret: number }>;
}

export type VimEffect = "apply" | undefined;

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

function charClass(c: string): 0 | 1 | 2 {
  if (/\s/.test(c)) return 0;
  if (/\w/.test(c)) return 1;
  return 2;
}
function wordForward(text: string, caret: number): number {
  const n = text.length;
  let i = caret;
  if (i >= n) return n;
  const cls = charClass(text[i]);
  if (cls !== 0) while (i < n && charClass(text[i]) === cls) i++;
  while (i < n && charClass(text[i]) === 0) i++;
  return i;
}
function wordBackward(text: string, caret: number): number {
  let i = caret - 1;
  while (i >= 0 && charClass(text[i]) === 0) i--;
  if (i < 0) return 0;
  const cls = charClass(text[i]);
  while (i >= 0 && charClass(text[i]) === cls) i--;
  return i + 1;
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

  if (st.op) return handleOperator(e, text, caret, st); // filled in Task 6

  // pending "g" (for gg)
  if (st.pending === "g") {
    const cleared = { ...st, pending: "" };
    if (e.key === "g") return done(text, 0, cleared);
    return handleNormal(e, text, caret, cleared);
  }

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
    case "w":
      return done(text, wordForward(text, caret), st);
    case "b":
      return done(text, wordBackward(text, caret), st);
    case "i":
      return done(text, caret, snapshot({ ...st, mode: "insert" }, text, caret));
    case "a":
      return done(text, Math.min(le, caret + 1), snapshot({ ...st, mode: "insert" }, text, caret));
    case "o": {
      const st2 = snapshot({ ...st, mode: "insert" }, text, caret);
      return done(text.slice(0, le) + "\n" + text.slice(le), le + 1, st2);
    }
    case "O": {
      const st2 = snapshot({ ...st, mode: "insert" }, text, caret);
      return done(text.slice(0, ls) + "\n" + text.slice(ls), ls, st2);
    }
    case "Escape":
      return done(text, caret, { ...st, pending: "", op: null });
    case "d":
    case "c":
    case "y":
      return done(text, caret, { ...st, op: e.key as "d" | "c" | "y" });
    default:
      return editKeys(e, text, caret, st); // x/dd/yy/p/u etc., filled in Tasks 5/7
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

// --- stubs filled in later tasks -------------------------------------------

function editKeys(e: KeyInput, text: string, caret: number, st: VimState): VimResult {
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
  op: "d" | "c" | "y",
  pending: string,
): Span | null | "await" {
  if (pending === "g") {
    if (e.key === "g") return linewiseSpan(text, caret, 0); // dgg
    return null; // abort
  }
  switch (e.key) {
    case op:
      return linewiseSpan(text, caret, caret); // dd/cc/yy
    case "g":
      return "await";
    case "G":
      return linewiseSpan(text, caret, text.length);
    case "w":
      return { from: caret, to: wordForward(text, caret), linewise: false };
    case "b":
      return { from: wordBackward(text, caret), to: caret, linewise: false };
    case "0":
      return { from: lineStartOf(text, caret), to: caret, linewise: false };
    case "$":
      return { from: caret, to: lineEndOf(text, caret), linewise: false };
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
  if (span === "await") return done(text, caret, { ...st, pending: "g" });
  if (span === null) return done(text, caret, { ...st, op: null, pending: "" });

  const { from, to, linewise } = span;
  const cleared = { ...st, op: null, pending: "" };

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
  return done(text, caret, { ...st, mode: "normal", exBuf: "" });
}
function handleSearch(e: KeyInput, text: string, caret: number, st: VimState): VimResult {
  return done(text, caret, { ...st, mode: "normal", searchBuf: "" });
}
