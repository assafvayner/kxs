import { describe, expect, it } from "vitest";
import { EditorState, Text } from "@codemirror/state";
import { filterHighlight, matchingLines, setFilter } from "./filterHighlight";

const doc = Text.of(["apiVersion: v1", "kind: Pod", "metadata:", "  name: web-0", "  namespace: prod"]);

describe("matchingLines", () => {
  it("returns no lines for an empty or blank filter", () => {
    expect(matchingLines(doc, "")).toEqual([]);
    expect(matchingLines(doc, "   ")).toEqual([]);
  });

  it("returns 1-based line numbers of lines matching the filter", () => {
    expect(matchingLines(doc, "name")).toEqual([4, 5]);
  });

  it("supports the -r regex prefix", () => {
    expect(matchingLines(doc, "-r ^kind")).toEqual([2]);
  });

  it("returns nothing when no line matches", () => {
    expect(matchingLines(doc, "zzz")).toEqual([]);
  });
});

function highlightedFroms(state: EditorState): number[] {
  const out: number[] = [];
  state.field(filterHighlight).deco.between(0, state.doc.length, (from) => {
    out.push(from);
  });
  return out;
}

describe("filterHighlight", () => {
  const text = "apiVersion: v1\nkind: Pod\nmetadata:\n  name: web-0\n  namespace: prod";

  it("starts with no decorations", () => {
    const state = EditorState.create({ doc: text, extensions: [filterHighlight] });
    expect(highlightedFroms(state)).toEqual([]);
  });

  it("decorates matching lines when a filter is set", () => {
    let state = EditorState.create({ doc: text, extensions: [filterHighlight] });
    state = state.update({ effects: setFilter.of("name") }).state;
    expect(highlightedFroms(state)).toEqual([state.doc.line(4).from, state.doc.line(5).from]);
  });

  it("recomputes on document change while a filter is active", () => {
    let state = EditorState.create({ doc: text, extensions: [filterHighlight] });
    state = state.update({ effects: setFilter.of("kind") }).state;
    state = state.update({ changes: { from: 0, insert: "kind: Deployment\n" } }).state;
    expect(highlightedFroms(state)).toEqual([state.doc.line(1).from, state.doc.line(3).from]);
  });

  it("applies a filter change and a doc change from the same transaction", () => {
    let state = EditorState.create({ doc: text, extensions: [filterHighlight] });
    state = state.update({
      changes: { from: state.doc.length, insert: "\nstatus: Running" },
      effects: setFilter.of("status"),
    }).state;
    expect(highlightedFroms(state)).toEqual([state.doc.line(6).from]);
  });

  it("clears decorations when the filter is emptied", () => {
    let state = EditorState.create({ doc: text, extensions: [filterHighlight] });
    state = state.update({ effects: setFilter.of("name") }).state;
    state = state.update({ effects: setFilter.of("") }).state;
    expect(highlightedFroms(state)).toEqual([]);
  });
});
