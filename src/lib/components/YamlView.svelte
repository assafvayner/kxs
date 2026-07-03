<script lang="ts">
  import { api, type ResourceKind } from "../api";
  import { tick } from "svelte";
  import { settings } from "../stores/settings.svelte";
  import { initialVimState, vimKey, type VimState } from "../vim";

  let {
    tabId,
    title,
    body,
    editable,
  }: {
    tabId: number;
    title: string;
    body: string;
    editable?: { resourceKind: ResourceKind; namespace: string | null; name: string };
  } = $props();

  let query = $state("");
  const lines = $derived(body.split("\n"));
  const matches = $derived(
    query ? lines.reduce((n, l) => n + (l.toLowerCase().includes(query.toLowerCase()) ? 1 : 0), 0) : 0,
  );

  // svelte-ignore state_referenced_locally -- one-time seed of the editable draft from the initial prop value
  let draft = $state(body);
  // svelte-ignore state_referenced_locally -- one-time seed of the saved baseline from the initial prop value
  let saved = $state(body);
  let dirty = $derived(draft !== saved);
  let status = $state<{ kind: "idle" | "ok" | "err"; msg: string }>({ kind: "idle", msg: "" });
  let busy = $state(false);

  let ta: HTMLTextAreaElement | undefined = $state();
  let vimState = $state<VimState>(initialVimState());
  const vimOn = $derived(!!editable && settings.vimMode);

  $effect(() => {
    // reset the engine whenever vim mode turns on
    if (vimOn) vimState = initialVimState();
  });

  function vimStatus(s: VimState): string {
    if (s.mode === "ex") return ":" + s.exBuf;
    if (s.mode === "search") return "/" + s.searchBuf;
    return s.mode === "insert" ? "-- INSERT --" : "-- NORMAL --";
  }

  async function onEditorKeydown(e: KeyboardEvent) {
    if (!vimOn || !ta) return;
    const caret = ta.selectionStart ?? 0;
    const r = vimKey(e, draft, caret, vimState);
    vimState = r.state;
    if (!r.handled) return; // native typing / shortcuts (insert mode, arrows, Cmd+…)
    e.preventDefault();
    if (r.text !== draft) draft = r.text;
    await tick();
    if (!ta) return;
    ta.selectionStart = ta.selectionEnd = r.caret;
    if (r.effect === "apply" && !busy && dirty) apply();
  }

  async function validate() {
    if (!editable) return;
    busy = true;
    try {
      await api.applyYaml(tabId, editable.resourceKind, editable.namespace, editable.name, draft, true);
      status = { kind: "ok", msg: "Valid (dry-run passed)" };
    } catch (e) {
      status = { kind: "err", msg: String(e) };
    } finally {
      busy = false;
    }
  }
  async function apply() {
    if (!editable) return;
    busy = true;
    try {
      await api.applyYaml(tabId, editable.resourceKind, editable.namespace, editable.name, draft, false);
      saved = draft;
      status = { kind: "ok", msg: "Applied" };
    } catch (e) {
      status = { kind: "err", msg: String(e) };
    } finally {
      busy = false;
    }
  }
</script>

<div class="detail">
  <div class="detail-bar">
    <span class="mono">{title}</span>
    {#if editable}
      <button onclick={validate} disabled={busy}>Validate</button>
      <button class="primary" onclick={apply} disabled={busy || !dirty}>Apply</button>
      {#if dirty}<span class="dim">● unsaved</span>{/if}
      {#if vimOn}<span class="vim-mode mono" aria-live="polite">{vimStatus(vimState)}</span>{/if}
      {#if status.kind === "ok"}<span class="st-ok">{status.msg}</span>{/if}
      {#if status.kind === "err"}<span class="st-bad" title={status.msg}>apply failed</span>{/if}
    {:else}
      <input class="mono" placeholder="/ search" bind:value={query} />
      {#if query}<span class="dim">{matches} matching lines</span>{/if}
    {/if}
  </div>
  {#if editable}
    <textarea
      class="yaml-editor mono"
      bind:this={ta}
      bind:value={draft}
      spellcheck="false"
      onkeydown={onEditorKeydown}></textarea>
    {#if status.kind === "err"}<pre class="apply-err mono">{status.msg}</pre>{/if}
  {:else}
    <pre class="detail-body mono">{#each lines as l}<div class:hl={query && l.toLowerCase().includes(query.toLowerCase())}>{l || " "}</div>{/each}</pre>
  {/if}
</div>
