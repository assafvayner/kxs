<script lang="ts">
  import type { TabSession } from "../stores/sessions.svelte";

  let { session, enabled }: { session: TabSession; enabled: boolean } = $props();

  // svelte-ignore state_referenced_locally -- one-time seed from the session's current filter
  let text = $state(session.filter);
  let input: HTMLInputElement | undefined = $state();

  export function focus() {
    input?.focus();
    input?.select();
  }

  $effect(() => {
    const t = text;
    const id = setTimeout(() => {
      session.filter = t;
    }, 100);
    return () => clearTimeout(id);
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      input?.blur();
    }
  }
  function clear() {
    text = "";
    session.filter = "";
    input?.focus();
  }
</script>

<div class="searchbar" class:disabled={!enabled}>
  <span class="mag">🔍</span>
  <input
    bind:this={input}
    bind:value={text}
    {onkeydown}
    disabled={!enabled}
    placeholder="search (-r for regex)"
    class="mono" />
  {#if text}
    <button type="button" class="clear" onclick={clear} title="clear">×</button>
  {/if}
</div>
