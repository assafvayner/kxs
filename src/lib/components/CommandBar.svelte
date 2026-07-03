<script lang="ts">
  import type { TabSession } from "../stores/sessions.svelte";
  import { fuzzyKinds, resolveKind } from "../command";
  import type { ResourceKind } from "../api";

  let { session, onclose, onpick, appCommands }: {
    session: TabSession;
    onclose: () => void;
    onpick: (k: ResourceKind) => void;
    /** Exact-match app-level commands (e.g. "pf", "top") checked before resource-kind resolution. */
    appCommands?: Record<string, () => void>;
  } = $props();

  let text = $state("");
  let input: HTMLInputElement | undefined = $state();
  const suggestions = $derived(fuzzyKinds(session.kinds, text).slice(0, 8));

  $effect(() => {
    input?.focus();
  });

  function submit(e: Event) {
    e.preventDefault();
    const cmd = appCommands?.[text.trim().toLowerCase()];
    if (cmd) {
      cmd();
      onclose();
      return;
    }
    const k = resolveKind(session.kinds, text) ?? suggestions[0];
    if (k) {
      onpick(k);
      onclose();
    }
  }
  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }
</script>

<form class="cmdbar" onsubmit={submit}>
  <span class="sigil">:</span>
  <input
    bind:this={input}
    bind:value={text}
    {onkeydown}
    placeholder="resource (po, svc, deploy…)"
    class="mono" />
  {#if suggestions.length}
    <div class="suggest">
      {#each suggestions as k (k.group + "/" + k.kind)}
        <button type="button" class="sug" onclick={() => { onpick(k); onclose(); }}>
          <span>{k.kind}</span><span class="dim">{k.group || "core"}</span>
        </button>
      {/each}
    </div>
  {/if}
</form>
