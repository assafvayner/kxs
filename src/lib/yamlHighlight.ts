/**
 * Pure, line-based YAML tokenizer for read-only syntax highlighting.
 *
 * Intentionally conservative: anything that isn't confidently classified is
 * left unstyled (no token emitted for that span) rather than risk a
 * misleading color.
 */

export type TokenClass = "key" | "str" | "num" | "cmt" | "docsep" | "dash" | "anchor" | "tag";

export interface YamlToken {
  start: number;
  end: number;
  cls: TokenClass;
}

const KEY_RE = /^("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|[^\s:#][^:]*?):(?=\s|$)/;

const NUM_RE = /^[+-]?(\d+\.\d*|\.\d+|\d+)([eE][+-]?\d+)?$/;
const HEX_RE = /^[+-]?0x[0-9a-fA-F]+$/;
const OCT_RE = /^[+-]?0o[0-7]+$/;
const SPECIAL_NUM_RE = /^[+-]?\.(inf|Inf|INF)$|^\.(nan|NaN|NAN)$/;
const TIMESTAMP_RE = /^\d{4}-\d{2}-\d{2}([Tt ]\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:?\d{2})?)?$/;
const BOOL_NULL_SET = new Set([
  "true", "True", "TRUE", "false", "False", "FALSE",
  "null", "Null", "NULL", "~",
  "yes", "Yes", "YES", "no", "No", "NO",
  "on", "On", "ON", "off", "Off", "OFF",
]);

function isNumLike(s: string): boolean {
  return (
    NUM_RE.test(s) ||
    HEX_RE.test(s) ||
    OCT_RE.test(s) ||
    SPECIAL_NUM_RE.test(s) ||
    TIMESTAMP_RE.test(s) ||
    BOOL_NULL_SET.has(s)
  );
}

function leadingSpaces(line: string): number {
  let i = 0;
  while (i < line.length && line[i] === " ") i++;
  return i;
}

/** Index of an inline `#` comment starting at/after `from`, or -1. Requires the `#`
 * be at `from` itself or preceded by whitespace, per YAML's comment rule. */
function commentStart(line: string, from: number): number {
  for (let i = from; i < line.length; i++) {
    if (line[i] === "#" && (i === from || line[i - 1] === " ")) return i;
  }
  return -1;
}

/** End index (exclusive) of a quoted scalar starting at `start` (line[start] is the quote). */
function scanQuoted(line: string, start: number): number {
  const q = line[start];
  let i = start + 1;
  while (i < line.length) {
    if (q === '"' && line[i] === "\\") {
      i += 2;
      continue;
    }
    if (line[i] === q) {
      if (q === "'" && line[i + 1] === "'") {
        i += 2;
        continue;
      }
      return i + 1;
    }
    i++;
  }
  return line.length;
}

function scanWord(line: string, start: number): number {
  let i = start;
  while (i < line.length && !/\s/.test(line[i])) i++;
  return i;
}

function tokenizeValue(line: string, start: number, tokens: YamlToken[]): void {
  const len = line.length;
  let i = start;

  while (i < len && (line[i] === "&" || line[i] === "*" || line[i] === "!")) {
    const end = scanWord(line, i);
    tokens.push({ start: i, end, cls: line[i] === "!" ? "tag" : "anchor" });
    i = end;
    while (i < len && line[i] === " ") i++;
    if (i >= len) return;
    if (line[i] === "#") {
      tokens.push({ start: i, end: len, cls: "cmt" });
      return;
    }
  }
  if (i >= len) return;

  if (line[i] === '"' || line[i] === "'") {
    const end = scanQuoted(line, i);
    tokens.push({ start: i, end, cls: "str" });
    let j = end;
    while (j < len && line[j] === " ") j++;
    const cs = commentStart(line, j);
    if (cs !== -1) tokens.push({ start: cs, end: len, cls: "cmt" });
    return;
  }

  // flow collections (`{}`, `[]`, or nested) and block-scalar indicators (`|`, `>`)
  // are left unstyled: robust-but-plain beats a confidently wrong parse.
  if (line[i] === "{" || line[i] === "[" || line[i] === "|" || line[i] === ">") return;

  const cs = commentStart(line, i);
  const valEnd = cs === -1 ? len : cs;
  let ve = valEnd;
  while (ve > i && line[ve - 1] === " ") ve--;
  if (ve > i) {
    const raw = line.slice(i, ve);
    tokens.push({ start: i, end: ve, cls: isNumLike(raw) ? "num" : "str" });
  }
  if (cs !== -1) tokens.push({ start: cs, end: len, cls: "cmt" });
}

/** When `line` opens a block scalar (`key: |`, `- >-`, ...), returns the column
 * its content must be indented past (the column of the key/dash content itself,
 * not the leading whitespace) so continuation lines can be recognized. */
function blockScalarStart(line: string): number | null {
  const len = line.length;
  let i = leadingSpaces(line);
  if (i >= len) return null;
  while (line[i] === "-" && (i + 1 === len || line[i + 1] === " ")) {
    i++;
    while (i < len && line[i] === " ") i++;
  }
  if (i >= len) return null;
  const contentCol = i;
  const keyMatch = line.slice(i).match(KEY_RE);
  if (keyMatch) {
    i += keyMatch[0].length;
    while (i < len && line[i] === " ") i++;
  }
  if (i >= len) return null;
  const opens = (line[i] === "|" || line[i] === ">") && /^[|>][+-]?\d*\s*(#.*)?$/.test(line.slice(i));
  return opens ? contentCol : null;
}

/** Tokenizes a single line with no cross-line context (block-scalar continuation
 * lines are handled by {@link tokenizeYamlDocument}, not this function). */
export function tokenizeYamlLine(line: string): YamlToken[] {
  const tokens: YamlToken[] = [];
  const len = line.length;
  let i = leadingSpaces(line);
  if (i >= len) return tokens;

  if (
    line.slice(i, i + 3) === "---" &&
    (i + 3 === len || line[i + 3] === " " || line[i + 3] === "#")
  ) {
    tokens.push({ start: i, end: i + 3, cls: "docsep" });
    const cs = commentStart(line, i + 3);
    if (cs !== -1) tokens.push({ start: cs, end: len, cls: "cmt" });
    return tokens;
  }
  if (line.slice(i, i + 3) === "..." && i + 3 === len) {
    tokens.push({ start: i, end: i + 3, cls: "docsep" });
    return tokens;
  }
  if (line[i] === "#") {
    tokens.push({ start: i, end: len, cls: "cmt" });
    return tokens;
  }

  while (line[i] === "-" && (i + 1 === len || line[i + 1] === " ")) {
    tokens.push({ start: i, end: i + 1, cls: "dash" });
    i++;
    while (i < len && line[i] === " ") i++;
    if (i >= len) return tokens;
    if (line[i] === "#") {
      tokens.push({ start: i, end: len, cls: "cmt" });
      return tokens;
    }
  }
  if (i >= len) return tokens;

  const keyMatch = line.slice(i).match(KEY_RE);
  if (keyMatch) {
    const keyEnd = i + keyMatch[0].length;
    tokens.push({ start: i, end: keyEnd, cls: "key" });
    i = keyEnd;
    while (i < len && line[i] === " ") i++;
    if (i >= len) return tokens;
    if (line[i] === "#") {
      tokens.push({ start: i, end: len, cls: "cmt" });
      return tokens;
    }
    tokenizeValue(line, i, tokens);
    return tokens;
  }

  tokenizeValue(line, i, tokens);
  return tokens;
}

/**
 * Tokenizes a full YAML document, tracking block-scalar (`|`/`>`) continuation
 * lines across lines so their content is highlighted as plain string rather
 * than being (mis)parsed as keys/values.
 */
export function tokenizeYamlDocument(text: string): YamlToken[][] {
  const lines = text.split("\n");
  const out: YamlToken[][] = [];
  let blockIndent: number | null = null;

  for (const line of lines) {
    const indent = leadingSpaces(line);
    const isBlank = indent === line.length;

    if (blockIndent !== null) {
      if (isBlank || indent > blockIndent) {
        out.push(isBlank ? [] : [{ start: indent, end: line.length, cls: "str" }]);
        continue;
      }
      blockIndent = null;
    }

    out.push(tokenizeYamlLine(line));
    const opensBlock = blockScalarStart(line);
    if (opensBlock !== null) blockIndent = opensBlock;
  }

  return out;
}

export interface YamlSegment {
  text: string;
  cls: TokenClass | null;
}

/** Fills the gaps between (and around) a line's tokens with unstyled segments,
 * so the concatenation of all segments reproduces `line` exactly. */
export function yamlLineSegments(line: string, tokens: YamlToken[]): YamlSegment[] {
  const segments: YamlSegment[] = [];
  let pos = 0;
  for (const t of tokens) {
    if (t.start > pos) segments.push({ text: line.slice(pos, t.start), cls: null });
    segments.push({ text: line.slice(t.start, t.end), cls: t.cls });
    pos = t.end;
  }
  if (pos < line.length) segments.push({ text: line.slice(pos), cls: null });
  if (segments.length === 0) segments.push({ text: line, cls: null });
  return segments;
}
