export interface CursorContext {
  mode: "key" | "value";
  /** Keys and array indexes from the root to the map that contains the cursor. */
  path: Array<string | number>;
  /** Offset where the word being typed starts. */
  from: number;
  word: string;
  /** In value mode, the key whose value is being typed. */
  key?: string;
  /** Keys already present in the containing map. */
  siblings: string[];
  /** Column of keys in the containing map. */
  indent: number;
}

interface Entry {
  indent: number;
  dash: boolean;
  key: string | null;
}

const KEY = "[A-Za-z0-9_./-]+";
const KEY_LINE = new RegExp(`^(${KEY})\\s*:(?:\\s|$)`);

function entriesOf(line: string): Entry[] {
  if (!line.trim() || /^\s*#/.test(line)) return [];
  const indent = line.match(/^ */)![0].length;
  let text = line.slice(indent);
  const out: Entry[] = [];
  let keyIndent = indent;
  if (text === "-" || text.startsWith("- ")) {
    out.push({ indent, dash: true, key: null });
    text = text.slice(2).trimStart();
    keyIndent = indent + 2;
  }
  const m = text.match(KEY_LINE);
  if (m) out.push({ indent: keyIndent, dash: false, key: m[1] });
  return out;
}

export function cursorContext(text: string, pos: number): CursorContext | null {
  const lineStart = text.lastIndexOf("\n", pos - 1) + 1;
  const nl = text.indexOf("\n", pos);
  const lineEnd = nl === -1 ? text.length : nl;
  const before = text.slice(lineStart, pos);
  const indent = before.match(/^ */)![0].length;
  let rest = before.slice(indent);
  let dash = false;
  if (rest === "-" || rest.startsWith("- ")) {
    dash = true;
    rest = rest.slice(2).trimStart();
  }
  const effIndent = dash ? indent + 2 : indent;

  let mode: "key" | "value";
  let word: string;
  let key: string | undefined;
  const keyOnly = rest.match(new RegExp(`^(${KEY}|)$`));
  const keyValue = rest.match(new RegExp(`^(${KEY})\\s*:\\s+(${KEY}|)$`));
  if (keyOnly) {
    mode = "key";
    word = keyOnly[1];
  } else if (keyValue) {
    mode = "value";
    key = keyValue[1];
    word = keyValue[2];
  } else {
    return null;
  }

  const above = text.slice(0, lineStart).split("\n").flatMap(entriesOf);
  if (dash) above.push({ indent, dash: true, key: null });
  const path = parentPath(above, effIndent);

  const siblings: string[] = [];
  if (mode === "key") {
    for (let i = above.length - 1; i >= 0 && above[i].indent >= effIndent; i--) {
      if (above[i].indent === effIndent && above[i].key) siblings.unshift(above[i].key!);
    }
    const below = text.slice(lineEnd + 1).split("\n").flatMap(entriesOf);
    for (const e of below) {
      if (e.indent < effIndent) break;
      if (e.indent === effIndent && e.key) siblings.push(e.key);
    }
  }

  return { mode, path, from: pos - word.length, word, key, siblings, indent: effIndent };
}

/** Walks upward: each entry shallower than the current level is a parent key or a sequence item. */
function parentPath(above: Entry[], startIndent: number): Array<string | number> {
  const path: Array<string | number> = [];
  let cur = startIndent;
  let i = above.length - 1;
  while (i >= 0) {
    const e = above[i];
    if (e.indent < cur) {
      if (e.dash) {
        let index = 0;
        let j = i - 1;
        while (j >= 0 && above[j].indent >= e.indent) {
          if (above[j].indent === e.indent && above[j].dash) index++;
          j--;
        }
        path.unshift(index);
        cur = e.indent;
        i = j;
        continue;
      }
      if (e.key) {
        path.unshift(e.key);
        cur = e.indent;
      }
    }
    i--;
  }
  return path;
}
