<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, type RolloutRevision } from "../api";
  import type { TabSession } from "../stores/sessions.svelte";
  import { now } from "../stores/now.svelte";
  import { age } from "../age";
  import { matchRow } from "../command";

  let {
    tabId,
    namespace,
    name,
    session,
    onundo,
  }: {
    tabId: number;
    namespace: string;
    name: string;
    session: TabSession;
    onundo: (revision: number) => void;
  } = $props();

  let revisions = $state<RolloutRevision[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let timer: ReturnType<typeof setInterval> | undefined;

  const COLS = "0.6fr 1.6fr 2.4fr 0.6fr 0.8fr";

  async function refresh() {
    try {
      revisions = await api.rolloutHistory(tabId, namespace, name);
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  const visible = $derived(
    revisions.filter((r) => matchRow(`${r.revision} ${r.name} ${r.images.join(" ")}`, session.filter)),
  );

  onMount(() => {
    refresh();
    timer = setInterval(refresh, 5000);
  });
  onDestroy(() => clearInterval(timer));
</script>

<div class="rtable">
  {#if error}
    <div class="connect-error"><pre class="mono">{error}</pre></div>
  {:else}
    <div class="rtable-head" style="grid-template-columns: {COLS};">
      <span>Revision</span><span>ReplicaSet</span><span>Images</span><span>Age</span><span></span>
    </div>
    {#each visible as rev (rev.revision)}
      <div class="rtable-row" style="grid-template-columns: {COLS};">
        <span>{rev.revision}{#if rev.current}&nbsp;<span class="st-ok">(current)</span>{/if}</span>
        <span class="mono dim">{rev.name}</span>
        <span class="mono" title={rev.images.join(", ")}>{rev.images.join(", ")}</span>
        <span>{age(rev.created, now.ms)}</span>
        <span>
          {#if !rev.current}
            <button onclick={() => onundo(rev.revision)}>Roll back</button>
          {/if}
        </span>
      </div>
    {/each}
    {#if loading && revisions.length === 0}
      <p class="dim pad">Loading…</p>
    {:else if revisions.length === 0}
      <p class="dim pad">No revision history for {namespace}/{name}.</p>
    {/if}
  {/if}
</div>
