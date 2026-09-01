import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { yaml } from "@codemirror/lang-yaml";
import { indentUnit, syntaxHighlighting } from "@codemirror/language";
import { search, searchKeymap } from "@codemirror/search";
import { EditorState, type Extension } from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { Vim } from "@replit/codemirror-vim";
import { filterHighlight } from "./filterHighlight";
import { editorTheme, yamlHighlight } from "./theme";
import { EX_COMMANDS, runEx } from "./vimEx";

let exDefined = false;
/** Vim.defineEx is global; register our ex commands once per app. */
export function defineExCommands(): void {
  if (exDefined) return;
  exDefined = true;
  for (const [name, prefix] of EX_COMMANDS) {
    Vim.defineEx(name, prefix, (cm) => {
      runEx(cm.cm6, name);
    });
  }
}

export interface EditorOptions {
  readOnly: boolean;
  onChange: (doc: string) => void;
}

/** Everything except the vim and Escape bindings, which the component swaps through compartments. */
export function buildExtensions(o: EditorOptions): Extension[] {
  const editing: Extension[] = o.readOnly
    ? []
    : [history(), keymap.of([...historyKeymap, indentWithTab]), highlightActiveLine(), highlightActiveLineGutter()];
  return [
    lineNumbers(),
    drawSelection(),
    highlightSpecialChars(),
    EditorState.readOnly.of(o.readOnly),
    EditorState.tabSize.of(2),
    indentUnit.of("  "),
    yaml(),
    syntaxHighlighting(yamlHighlight),
    editorTheme,
    filterHighlight,
    search({ top: true }),
    keymap.of([...searchKeymap, ...defaultKeymap]),
    EditorView.updateListener.of((u) => {
      if (u.docChanged) o.onChange(u.state.doc.toString());
    }),
    ...editing,
  ];
}
