<script lang="ts">
  import { onMount } from "svelte";
  import { Compartment, EditorState, StateEffect, type Extension } from "@codemirror/state";
  import { EditorView, keymap, ViewPlugin } from "@codemirror/view";
  import { closeSearchPanel } from "@codemirror/search";
  import { getCM, vim } from "@replit/codemirror-vim";
  import type { K8sOptions } from "../editor/k8s";
  import { matchingLines, setFilter } from "../editor/filterHighlight";
  import { buildExtensions, defineExCommands } from "../editor/setup";
  import { editorThemeDark, editorThemeLight } from "../editor/theme";
  import { setExCommands, type ExCommands } from "../editor/vimEx";
  import { settings } from "../stores/settings.svelte";

  let {
    value = $bindable(""),
    readOnly = false,
    vim: vimOn = false,
    filter = "",
    autofocus = false,
    commands,
    onEscape,
    onVimMode,
    k8s,
  }: {
    value?: string;
    /** Read at construction only. */
    readOnly?: boolean;
    vim?: boolean;
    filter?: string;
    autofocus?: boolean;
    commands?: ExCommands;
    /** Escape when vim is off. Defaults to blurring the editor so app shortcuts resume. */
    onEscape?: () => void;
    /** Vim mode name: normal, insert, visual, visual line, visual block, replace. */
    onVimMode?: (mode: string) => void;
    /** Read at construction only. */
    k8s?: K8sOptions;
  } = $props();

  let host: HTMLDivElement | undefined = $state();
  let view: EditorView | undefined;
  const vimCompartment = new Compartment();
  const escapeCompartment = new Compartment();
  const themeCompartment = new Compartment();
  let lastDark = settings.effectiveTheme.dark;

  // Deliberate policy: with vim on, paste is insert-mode only (no implicit mode switch).
  // The vim keymap's own bubble-phase paste listener would flip to insert mode, so this
  // runs in the capture phase ahead of it.
  const vimPasteGuard = ViewPlugin.define((v) => {
    const onPaste = (e: ClipboardEvent) => {
      if (getCM(v)?.state.vim?.insertMode) return;
      e.preventDefault();
      e.stopImmediatePropagation();
    };
    v.contentDOM.addEventListener("paste", onPaste, true);
    return { destroy: () => v.contentDOM.removeEventListener("paste", onPaste, true) };
  });

  function vimExtension(on: boolean): Extension {
    return on ? [vim(), vimPasteGuard] : [];
  }

  function escapeExtension(on: boolean): Extension {
    if (on) return [];
    return keymap.of([
      {
        key: "Escape",
        run: (v) => {
          if (closeSearchPanel(v)) return true;
          if (onEscape) onEscape();
          else v.contentDOM.blur();
          return true;
        },
      },
    ]);
  }

  const SUB_MODES: Record<string, string> = { linewise: "line", blockwise: "block" };

  // The CM5 adapter persists while the vim plugin is active, so attach once per adapter.
  let listening: unknown = null;
  function listenVimMode(v: EditorView) {
    const cm = getCM(v);
    if (!cm || cm === listening) return;
    listening = cm;
    cm.on("vim-mode-change", (e: { mode: string; subMode?: string }) => {
      onVimMode?.(e.subMode ? `${e.mode} ${SUB_MODES[e.subMode] ?? e.subMode}` : e.mode);
    });
    onVimMode?.("normal");
  }

  onMount(() => {
    defineExCommands();
    const v = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          vimCompartment.of(vimExtension(vimOn)),
          escapeCompartment.of(escapeExtension(vimOn)),
          themeCompartment.of(settings.effectiveTheme.dark ? editorThemeDark : editorThemeLight),
          buildExtensions({
            readOnly,
            k8s,
            onChange: (doc) => {
              if (doc !== value) value = doc;
            },
          }),
        ],
      }),
    });
    view = v;
    if (vimOn) listenVimMode(v);
    if (autofocus) v.focus();
    return () => {
      setExCommands(v, null);
      v.destroy();
      view = undefined;
    };
  });

  $effect(() => {
    const next = value;
    if (!view) return;
    const current = view.state.doc.toString();
    if (next !== current) {
      const changes = { from: 0, to: current.length, insert: next };
      const length = view.state.update({ changes }).newDoc.length;
      view.dispatch({ changes, selection: { anchor: Math.min(view.state.selection.main.head, length) } });
    }
  });

  $effect(() => {
    const on = vimOn;
    if (!view) return;
    view.dispatch({
      effects: [vimCompartment.reconfigure(vimExtension(on)), escapeCompartment.reconfigure(escapeExtension(on))],
    });
    if (on) listenVimMode(view);
    else listening = null;
  });

  $effect(() => {
    const f = filter;
    if (!view) return;
    const effects: StateEffect<unknown>[] = [setFilter.of(f)];
    const first = matchingLines(view.state.doc, f)[0];
    if (first !== undefined) {
      effects.push(EditorView.scrollIntoView(view.state.doc.line(first).from, { y: "center" }));
    }
    view.dispatch({ effects });
  });

  $effect(() => {
    const c = commands;
    if (view) setExCommands(view, c ?? null);
  });

  $effect(() => {
    const dark = settings.effectiveTheme.dark;
    if (view && dark !== lastDark) {
      lastDark = dark;
      view.dispatch({ effects: themeCompartment.reconfigure(dark ? editorThemeDark : editorThemeLight) });
    }
  });
</script>

<div class="code-editor" bind:this={host}></div>
