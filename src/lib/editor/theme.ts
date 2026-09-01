import { HighlightStyle } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";

const HL_BG = "#3b331c";

export const editorTheme = EditorView.theme(
  {
    "&": { height: "100%", backgroundColor: "var(--bg)", color: "var(--fg)", fontSize: "12px" },
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": {
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      lineHeight: "1.5",
      overflow: "auto",
    },
    ".cm-content": { padding: "12px 0", caretColor: "var(--fg)" },
    ".cm-line": { padding: "0 16px" },
    ".cm-gutters": { backgroundColor: "var(--bg)", color: "var(--fg-dim)", border: "none" },
    ".cm-lineNumbers .cm-gutterElement": { padding: "0 10px 0 8px", minWidth: "3ch" },
    ".cm-activeLine": { backgroundColor: "var(--bg-raised)" },
    ".cm-activeLineGutter": { backgroundColor: "var(--bg-raised)", color: "var(--fg)" },
    ".cm-cursor": { borderLeftColor: "var(--fg)" },
    "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
      { backgroundColor: "var(--bg-active)" },
    ".cm-hl": { backgroundColor: HL_BG },
    ".cm-searchMatch": { backgroundColor: "#2f334d", outline: "1px solid var(--yellow)" },
    ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "#5a4a1c" },
    ".cm-panels": { backgroundColor: "var(--bg-raised)", color: "var(--fg)" },
    ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--border)" },
    ".cm-panels.cm-panels-bottom": { borderTop: "1px solid var(--border)" },
    ".cm-panel input, .cm-panel button": { fontSize: "12px" },
    ".cm-vim-panel": {
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      padding: "4px 16px",
      alignItems: "center",
    },
    ".cm-vim-panel input": {
      color: "var(--fg)",
      fontFamily: "inherit",
      padding: "0 4px",
      border: "none",
      borderRadius: "0",
      background: "transparent",
    },
    ".cm-fat-cursor": { background: "var(--accent)", color: "var(--bg)" },
    "&:not(.cm-focused) .cm-fat-cursor": {
      background: "none",
      outline: "1px solid var(--accent)",
      color: "transparent",
    },
  },
  { dark: true },
);

export const yamlHighlight = HighlightStyle.define([
  { tag: t.definition(t.propertyName), color: "var(--accent)" },
  { tag: [t.string, t.special(t.string)], color: "var(--green)" },
  { tag: [t.comment, t.meta], color: "var(--fg-dim)", fontStyle: "italic" },
  { tag: [t.labelName, t.typeName], color: "var(--yellow)" },
  { tag: t.keyword, color: "var(--red)" },
  { tag: [t.separator, t.punctuation, t.squareBracket, t.brace], color: "var(--fg-dim)" },
  { tag: t.content, color: "var(--fg)" },
]);
