import { StateEffect, StateField, type Text } from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";
import { filterPredicate } from "../command";

/** 1-based numbers of the lines that match the SearchBar filter. */
export function matchingLines(doc: Text, filter: string): number[] {
  if (!filter.trim()) return [];
  const matches = filterPredicate(filter);
  const out: number[] = [];
  let n = 1;
  for (const line of doc.iterLines()) {
    if (matches(line)) out.push(n);
    n++;
  }
  return out;
}

export const setFilter = StateEffect.define<string>();

const hlLine = Decoration.line({ class: "cm-hl" });

function decorate(doc: Text, filter: string): DecorationSet {
  return Decoration.set(matchingLines(doc, filter).map((n) => hlLine.range(doc.line(n).from)));
}

export const filterHighlight = StateField.define<{ filter: string; deco: DecorationSet }>({
  create: () => ({ filter: "", deco: Decoration.none }),
  update(value, tr) {
    let filter = value.filter;
    let changed = false;
    for (const e of tr.effects) {
      if (e.is(setFilter)) {
        filter = e.value;
        changed = true;
      }
    }
    if (!changed && !tr.docChanged) return value;
    return { filter, deco: decorate(tr.state.doc, filter) };
  },
  provide: (f) => EditorView.decorations.from(f, (v) => v.deco),
});
