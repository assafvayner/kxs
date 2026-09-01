<script lang="ts">
  import { api, type ResourceKind } from "../api";
  import { onMount, tick } from "svelte";
  import { settings } from "../stores/settings.svelte";
  import { initialVimState, vimKey, type VimState } from "../vim";

  let {
    tabId,
    title,
    body,
    resourceKind,
    namespace,
    name,
    onClose,
  }: {
    tabId: number;
    title: string;
    body: string;
    resourceKind: ResourceKind;
    namespace: string | null;
    name: string;
    onClose: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally -- one-time seed of the draft from the initial prop value
  let draft = $state(body);
  // svelte-ignore state_referenced_locally -- one-time seed of the saved baseline from the initial prop value
  let saved = $state(body);
  let dirty = $derived(draft !== saved);
  let status = $state<{ kind: "idle" | "ok" | "err"; msg: string }>({ kind: "idle", msg: "" });
  let busy = $state(false);

  let ta: HTMLTextAreaElement | undefined = $state();
  let gutter: HTMLDivElement | undefined = $state();
  let vimState = $state<VimState>(initialVimState());
  const vimOn = $derived(settings.vimMode);
  const lineCount = $derived(draft.split("\n").length);
  const gutterDigits = $derived(String(lineCount).length);

  $effect(() => {
    // reset the engine whenever vim mode turns on
    if (vimOn) vimState = initialVimState();
  });

  onMount(() => ta?.focus());

  $effect(() => {
    // any edit invalidates a prior validate/apply status
    void draft;
    status = { kind: "idle", msg: "" };
  });

  function vimStatus(s: VimState): string {
    if (s.mode === "ex") return ":" + s.exBuf;
    if (s.mode === "search") return (s.searchBackward ? "?" : "/") + s.searchBuf;
    return s.mode === "insert" ? "-- INSERT --" : "-- NORMAL --";
  }

  function onEditorScroll() {
    if (gutter && ta) gutter.scrollTop = ta.scrollTop;
  }

  let charWidthCache: { font: string; width: number } | undefined;
  function charWidthOf(style: CSSStyleDeclaration): number {
    const font = style.font || `${style.fontSize} ${style.fontFamily}`;
    if (charWidthCache?.font !== font) {
      const ctx = document.createElement("canvas").getContext("2d");
      let width = 8;
      if (ctx) {
        ctx.font = font;
        width = ctx.measureText("0").width || 8;
      }
      charWidthCache = { font, width };
    }
    return charWidthCache.width;
  }

  function scrollCaretIntoView(el: HTMLTextAreaElement, text: string, caret: number) {
    const style = getComputedStyle(el);
    const lineHeight = parseFloat(style.lineHeight) || 16;
    const lineStart = text.lastIndexOf("\n", caret - 1) + 1;
    const line = text.slice(0, lineStart).split("\n").length - 1;
    const col = caret - lineStart;

    const top = line * lineHeight;
    if (top < el.scrollTop) el.scrollTop = top;
    else if (top + lineHeight > el.scrollTop + el.clientHeight) {
      el.scrollTop = top + lineHeight - el.clientHeight;
    }

    const charWidth = charWidthOf(style);
    const left = col * charWidth;
    const padRight = parseFloat(style.paddingRight) || 0;
    const visibleWidth = el.clientWidth - padRight;
    if (left < el.scrollLeft) el.scrollLeft = left;
    else if (left + charWidth > el.scrollLeft + visibleWidth) {
      el.scrollLeft = left + charWidth - visibleWidth;
    }
    onEditorScroll();
  }

  async function onEditorKeydown(e: KeyboardEvent) {
    if (!vimOn) {
      // no vim: Escape backs out of the editor (app convention)
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
      return;
    }
    if (!ta) return;
    const caret = ta.selectionStart ?? 0;
    const r = vimKey(e, draft, caret, vimState);
    vimState = r.state;
    if (!r.handled) return; // native typing / shortcuts (insert mode, arrows, Cmd+…)
    e.preventDefault();
    if (r.text !== draft) draft = r.text;
    await tick();
    if (!ta) return;
    if (r.effect !== "close" && r.effect !== "applyClose") {
      ta.selectionStart = ta.selectionEnd = r.caret;
      scrollCaretIntoView(ta, draft, r.caret);
    }
    if (r.effect === "apply") {
      if (!busy && dirty) apply();
    } else if (r.effect === "close") {
      onClose();
    } else if (r.effect === "applyClose") {
      if (!busy && dirty) {
        if (await apply()) onClose();
      } else {
        onClose();
      }
    }
  }

  async function validate() {
    busy = true;
    try {
      await api.applyYaml(tabId, resourceKind, namespace, name, draft, true);
      status = { kind: "ok", msg: "Valid (dry-run passed)" };
    } catch (e) {
      status = { kind: "err", msg: String(e) };
    } finally {
      busy = false;
    }
  }
  async function apply(): Promise<boolean> {
    busy = true;
    try {
      await api.applyYaml(tabId, resourceKind, namespace, name, draft, false);
      saved = draft;
      status = { kind: "ok", msg: "Applied" };
      return true;
    } catch (e) {
      status = { kind: "err", msg: String(e) };
      return false;
    } finally {
      busy = false;
    }
  }
</script>

<div class="detail">
  <div class="detail-bar">
    <span class="mono">{title}</span>
    <button onclick={validate} disabled={busy}>Validate</button>
    <button class="primary" onclick={apply} disabled={busy || !dirty}>Apply</button>
    {#if dirty}<span class="dim">● unsaved</span>{/if}
    {#if vimOn}<span class="vim-mode mono" aria-live="polite">{vimStatus(vimState)}</span>{/if}
    {#if status.kind === "ok"}<span class="st-ok">{status.msg}</span>{/if}
    {#if status.kind === "err"}<span class="st-bad" title={status.msg}>apply failed</span>{/if}
  </div>
  <div class="yaml-editor-wrap">
    <div
      class="yaml-gutter mono"
      bind:this={gutter}
      style="--gutter-digits: {gutterDigits}"
      aria-hidden="true"
    >
      {#each Array(lineCount) as _, i (i)}
        <div class="yaml-gutter-line">{i + 1}</div>
      {/each}
    </div>
    <textarea
      class="yaml-editor mono"
      bind:this={ta}
      bind:value={draft}
      spellcheck="false"
      wrap="off"
      onkeydown={onEditorKeydown}
      onscroll={onEditorScroll}
    ></textarea>
  </div>
  {#if status.kind === "err"}<pre class="apply-err mono">{status.msg}</pre>{/if}
</div>
