<script lang="ts">
  import type { TabSession } from "../stores/sessions.svelte";
  import { fuzzyKinds, resolveKind } from "../command";
  import type { ResourceKind } from "../api";

  let { session, mode, onclose, onpick }: {
    session: TabSession;
    mode: "command" | "filter";
    onclose: () => void;
    onpick: (k: ResourceKind) => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let text = $state(mode === "filter" ? session.filter : "");
  let input: HTMLInputElement | undefined = $state();
  const suggestions = $derived(mode === "command" ? fuzzyKinds(session.kinds, text).slice(0, 8) : []);

  $effect(() => {
    input?.focus();
  });

  function submit(e: Event) {
    e.preventDefault();
    if (mode === "filter") {
      session.filter = text;
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
      if (mode === "filter") session.filter = "";
      onclose();
    }
  }
</script>

<form class="cmdbar" onsubmit={submit}>
  <span class="sigil">{mode === "command" ? ":" : "/"}</span>
  <input
    bind:this={input}
    bind:value={text}
    {onkeydown}
    placeholder={mode === "command" ? "resource (po, svc, deploy…)" : "filter (-r for regex)"}
    class="mono" />
  {#if mode === "command" && suggestions.length}
    <div class="suggest">
      {#each suggestions as k (k.group + "/" + k.kind)}
        <button type="button" class="sug" onclick={() => { onpick(k); onclose(); }}>
          <span>{k.kind}</span><span class="dim">{k.group || "core"}</span>
        </button>
      {/each}
    </div>
  {/if}
</form>
