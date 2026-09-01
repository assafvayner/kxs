# CodeMirror 6 for the YAML views

## Summary

Replace the textarea-based YAML editor and the `<pre>`-based read-only YAML view
with CodeMirror 6. The editor gets a complete vim implementation from
`@replit/codemirror-vim`, both views get YAML syntax highlighting from
`@codemirror/lang-yaml`, and the hand-rolled vim engine in `src/lib/vim.ts` is
deleted.

## Goals

- Syntax-highlighted YAML in both the read-only view and the editor.
- Full vim in the editor (visual mode, text objects, counts, `.`, registers,
  marks, `:s`, regex search) without maintaining our own engine.
- Keep the existing app contracts: `:w` applies, `:q` closes, `:wq`/`:x` apply
  then close, Escape closes when vim is off, the vim mode indicator in the
  detail bar, the SearchBar filter highlighting lines in the read-only view.
- Keep bundle growth modest (target under 500 KB added, no workers). Measured:
  the main chunk went from 475 KB to 962 KB minified (293 KB gzip).
- Keep the app keyboard model intact: the read-only view must not steal keys
  (`/`, `:`, `j`/`k`, Escape) that the cluster tab handles today.

## Non-goals

- Kubernetes schema validation, completion, hover. Designed separately in
  `k8s-manifest-validation.md`; this change only has to leave room for it
  (extensions are composed in one place).
- Replacing `HighlightedText` in the describe view. Describe output is not
  YAML; it keeps the `<pre>`.
- Light theme. The app is dark-only today.

## Why CodeMirror 6 over Monaco

Monaco is 2 to 3 MB plus workers, ships its own theming system, and its vim
binding is a thin single-maintainer port. CodeMirror 6 is ESM, tree-shakable,
themed with plain CSS, and `@replit/codemirror-vim` is the same CodeMirror 5
vim keymap lineage, maintained by Replit. The state layer runs in Node, so the
parts that carry app logic stay unit-testable in vitest.

## Packages

| Package | Purpose |
|---|---|
| `@codemirror/state`, `@codemirror/view` | core |
| `@codemirror/language`, `@lezer/highlight` | highlighting, indentation |
| `@codemirror/lang-yaml` | YAML grammar |
| `@codemirror/commands` | default keymap, history, indent |
| `@codemirror/search` | Cmd+F panel in the editor |
| `@replit/codemirror-vim` | vim |

The `codemirror` meta package and `basicSetup` are not used. `basicSetup`
pulls in autocomplete, folding, bracket matching and lint UI we do not want by
default, and the exact extension list matters for keyboard behavior.

## Architecture

```
src/lib/editor/
  theme.ts            EditorView.theme + HighlightStyle bound to the app CSS vars
  filterHighlight.ts  StateField decorating lines that match the SearchBar filter
  vimEx.ts            ex-command registry (:w :q :wq :x) routed per EditorView
  setup.ts            buildExtensions({ readOnly, onChange }) -> Extension[]; defineExCommands()
src/lib/components/
  CodeEditor.svelte   thin Svelte wrapper owning one EditorView
  YamlView.svelte     read-only: <CodeEditor readOnly value filter>
  YamlEditView.svelte editor: <CodeEditor bind:value vim commands onVimMode>
```

### CodeEditor.svelte

Owns an `EditorView` created in `onMount` and destroyed in the cleanup. Props:

- `value` (bindable string). Doc-to-prop sync happens in an `updateListener`;
  prop-to-doc sync in an `$effect` that dispatches a replacing change only when
  the prop differs from the doc (the editor reseeds after Apply and Reload).
- `readOnly: boolean`. Read at construction only. Sets `EditorState.readOnly`
  and drops history and the editing keymaps. The view stays `editable` so
  selection and copy work.
- `vim: boolean`. Toggled through a `Compartment` so flipping the setting does
  not rebuild the view. The compartment holds the vim keymap plus a paste
  guard: a `ViewPlugin` with a capture-phase `paste` listener on `contentDOM`
  that cancels the event unless vim is in insert mode. It must be capture
  phase because the vim keymap's own bubble-phase paste listener on the same
  element would otherwise switch the editor into insert mode.
- `filter: string`. Forwarded to the filter-highlight StateField through a
  `StateEffect`; the first matching line is scrolled to center on change.
- `autofocus: boolean`. True for the editor, false for the read-only view.
- `commands: { write?, quit?, writeQuit? }`. Ex-command targets.
- `onEscape?: () => void`. The Escape binding is installed only when vim is
  off. It first closes an open search panel; otherwise it calls `onEscape` when
  given, or blurs the editor. The read-only view passes no `onEscape`, so
  Escape blurs it and app keys work again; the cluster tab's own Escape then
  pops the view.
- `onVimMode?: (mode: string) => void`. Fed from the `vim-mode-change` event of
  the CM5 adapter so the detail bar can show `-- NORMAL --`, `-- INSERT --`,
  `-- VISUAL --`, and so on.

### Key routing

The global window handler already ignores events whose target is content
editable, and CodeMirror's content element is `contenteditable`, so the editor
never leaks keys to app shortcuts. The read-only view is not focused on open, so
`/`, `:`, `j`/`k` and Escape keep reaching the cluster tab exactly as before. If
the user clicks into it, Escape blurs the editor and returns control.

### Ex commands

`Vim.defineEx` is global. `defineExCommands()` in `setup.ts` registers
`write`/`w`, `quit`/`q`, `wq`, and `x` once (module-level flag), called from
`CodeEditor`'s mount. `vimEx.ts` owns the name table, the
`WeakMap<EditorView, ExCommands>` registry, and `runEx`, which the ex
callbacks dispatch through. `:N`, `:d`,
`:y` with ranges, and `:s` come from the vim keymap itself and replace the
custom engine's partial ex support.

### Theme

One `EditorView.theme` referencing `var(--bg)`, `var(--fg)`, `var(--fg-dim)`,
`var(--accent)`, `var(--bg-active)` for chrome (gutter, active line, selection,
cursor, search panel, vim command line panel) and one `HighlightStyle`:

| Token | Color |
|---|---|
| key names | `--accent` |
| quoted strings, block scalars | `--green` |
| anchors, aliases, tags | `--yellow` |
| comments, document markers | `--fg-dim` |
| punctuation | `--fg-dim` |
| unquoted scalars | `--fg` |

The Lezer YAML grammar tags every unquoted scalar as plain content, so
numbers and booleans are not distinguished from other bare values.

Font size 12px and line height 1.5 match the current views.

### Filter highlighting

`filterHighlight.ts` exposes `matchingLines(doc: Text, filter: string)`
built on `filterPredicate` from `command.ts` (compiles the filter once;
`matchRow` is a thin wrapper over it), and a `StateField<{filter, deco}>` that
recomputes on doc change or a `setFilter` effect. Lines get
`Decoration.line({ class: "cm-hl" })`, styled like the current `.hltext .hl`.
Both the helper and the field are unit tested headlessly with `EditorState`.

## Tests

- `vim.ts` and `vim.test.ts` are deleted with the textarea editor.
- `filterHighlight.test.ts` covers `matchingLines` against `Text`
  documents, including regex filters and empty filters, and the `StateField`
  itself: decorations recompute on `setFilter` and on doc change, and stay
  untouched otherwise.
- `vimEx.test.ts` covers the registry: the right callback fires for each ex
  name, a view with no commands is a no-op, `:wq` and `:x` both route to
  `writeQuit`.
- `svelte-check` and the existing suite must stay green. Manual verification in
  `npm run tauri dev`: open a YAML view, filter it, edit it with vim on and off,
  `:w`, `:q`, `:wq`, Escape, Apply, Reload, and check the vim mode indicator.

## Migration steps

1. Add dependencies.
2. `theme.ts`, `filterHighlight.ts` (+ test), `vimEx.ts` (+ test), `setup.ts`.
3. `CodeEditor.svelte`.
4. Port `YamlView.svelte`.
5. Port `YamlEditView.svelte`; remove gutter and caret-scroll code.
6. Delete `vim.ts`, `vim.test.ts`, the `.yaml-editor*` and `.yaml-gutter*` CSS.
7. Build, check bundle size, manual verification.

## Risks

- Escape is only bound when vim is off, and that binding defers to the search
  panel's close first. Verified in a browser harness: Escape in vim normal mode
  neither closes nor blurs.
- `EditorState.readOnly` still lets vim enter insert mode visually. The
  read-only view does not enable vim, so this does not arise.
- Svelte 5 `bind:value` plus an external doc: the update listener and the
  prop effect must not ping-pong. Guard both directions with equality checks.
