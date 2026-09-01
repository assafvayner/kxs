import { HighlightStyle } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";

const themeSpec = {
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
  ".cm-hl": { backgroundColor: "var(--warn-bg)" },
  ".cm-searchMatch": { backgroundColor: "var(--bg-active)", outline: "1px solid var(--yellow)" },
  ".cm-searchMatch.cm-searchMatch-selected": {
    backgroundColor: "color-mix(in srgb, var(--yellow) 35%, var(--bg))",
  },
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
  ".cm-tooltip": { backgroundColor: "var(--bg-raised)", color: "var(--fg)", border: "1px solid var(--border)" },
  ".cm-tooltip.cm-tooltip-autocomplete > ul": { fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace", fontSize: "12px" },
  ".cm-tooltip.cm-tooltip-autocomplete > ul > li[aria-selected]": { backgroundColor: "var(--bg-active)", color: "var(--fg)" },
  ".cm-completionDetail": { color: "var(--fg-dim)", fontStyle: "normal", marginLeft: "1em" },
  ".cm-completionInfo": { maxWidth: "40em", whiteSpace: "pre-wrap" },
  ".cm-tooltip-lint": { fontSize: "12px" },
  ".cm-diagnostic": { borderLeftWidth: "3px" },
  ".cm-diagnostic-error": { borderLeftColor: "var(--red)" },
  ".cm-diagnostic-warning": { borderLeftColor: "var(--yellow)" },
  ".cm-diagnostic-info": { borderLeftColor: "var(--accent)" },
  ".cm-lintRange-error": { backgroundImage: "none", textDecoration: "underline wavy var(--red)", textUnderlineOffset: "3px" },
  ".cm-lintRange-warning": { backgroundImage: "none", textDecoration: "underline wavy var(--yellow)", textUnderlineOffset: "3px" },
  ".cm-lintRange-info": { backgroundImage: "none", textDecoration: "underline dotted var(--accent)", textUnderlineOffset: "3px" },
  ".cm-gutter-lint": { width: "1em" },
  ".cm-gutter-lint .cm-gutterElement": { padding: "0 0 0 2px" },
  ".cm-fat-cursor": { background: "var(--accent)", color: "var(--bg)" },
  "&:not(.cm-focused) .cm-fat-cursor": {
    background: "none",
    outline: "1px solid var(--accent)",
    color: "transparent",
  },
};

export const editorThemeDark = EditorView.theme(themeSpec, { dark: true });
export const editorThemeLight = EditorView.theme(themeSpec, { dark: false });

export const yamlHighlight = HighlightStyle.define([
  { tag: t.definition(t.propertyName), color: "var(--syn-key)" },
  { tag: [t.string, t.special(t.string)], color: "var(--syn-string)" },
  { tag: [t.comment, t.meta], color: "var(--syn-comment)", fontStyle: "italic" },
  { tag: [t.labelName, t.typeName], color: "var(--yellow)" },
  { tag: t.keyword, color: "var(--red)" },
  { tag: [t.separator, t.punctuation, t.squareBracket, t.brace], color: "var(--syn-punct)" },
  { tag: t.content, color: "var(--fg)" },
]);
