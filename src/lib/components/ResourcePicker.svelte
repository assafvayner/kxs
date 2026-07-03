<script lang="ts">
  import type { TabSession } from "../stores/sessions.svelte";
  import type { ResourceKind } from "../api";
  import { fuzzyKinds } from "../command";

  let { session, label, onpick }: {
    session: TabSession;
    label: string;
    onpick: (k: ResourceKind) => void;
  } = $props();

  let open = $state(false);
  let text = $state("");
  let input: HTMLInputElement | undefined = $state();
  let root: HTMLElement | undefined = $state();
  const matches = $derived(open ? fuzzyKinds(session.kinds, text).slice(0, 12) : []);

  $effect(() => {
    if (open) input?.focus();
  });

  function toggle() {
    open = !open;
    if (open) text = "";
  }
  function close() {
    open = false;
  }
  function pick(k: ResourceKind) {
    onpick(k);
    close();
  }
  function submit(e: Event) {
    e.preventDefault();
    const k = matches[0];
    if (k) pick(k);
  }
  function onInputKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }
  function onWindowClick(e: MouseEvent) {
    if (open && root && !root.contains(e.target as Node)) close();
  }
</script>

<svelte:window onclick={onWindowClick} />

<div class="respick" bind:this={root}>
  <button type="button" class="respick-btn mono" onclick={toggle}>
    {label} <span class="caret">▾</span>
  </button>
  {#if open}
    <form class="respick-panel" onsubmit={submit}>
      <input
        bind:this={input}
        bind:value={text}
        onkeydown={onInputKeydown}
        placeholder="resource (po, svc, deploy…)"
        class="mono" />
      <div class="respick-list">
        {#each matches as k (k.group + "/" + k.kind)}
          <button type="button" class="sug" onclick={() => pick(k)}>
            <span>{k.kind}</span><span class="dim">{k.group || "core"}</span>
          </button>
        {/each}
      </div>
    </form>
  {/if}
</div>
