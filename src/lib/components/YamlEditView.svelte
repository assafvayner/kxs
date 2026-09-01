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
  let vimState = $state<VimState>(initialVimState());
  const vimOn = $derived(settings.vimMode);

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
  <textarea
    class="yaml-editor mono"
    bind:this={ta}
    bind:value={draft}
    spellcheck="false"
    onkeydown={onEditorKeydown}></textarea>
  {#if status.kind === "err"}<pre class="apply-err mono">{status.msg}</pre>{/if}
</div>
