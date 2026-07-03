<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, type MetricsRow } from "../api";
  import type { TabSession } from "../stores/sessions.svelte";
  import VirtualList from "./VirtualList.svelte";

  let { tabId, session }: { tabId: number; session: TabSession } = $props();

  let rows = $state<MetricsRow[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let timer: ReturnType<typeof setInterval> | undefined;

  async function refresh() {
    try {
      const r = await api.podMetrics(tabId, session.namespace);
      rows = [...r].sort((a, b) => b.cpuMillicores - a.cpuMillicores);
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    timer = setInterval(refresh, 5000);
  });
  onDestroy(() => clearInterval(timer));

  // Reads session.namespace synchronously on each run, so this fires once on
  // mount and again whenever the tab's namespace changes.
  $effect(() => {
    refresh();
  });
</script>

<div class="rtable">
  {#if error}
    <div class="connect-error"><pre class="mono">{error}</pre></div>
  {:else}
    <div class="rtable-head" style="grid-template-columns: 1.5fr 2.5fr 1fr 1fr;">
      <span>Namespace</span><span>Pod</span><span>CPU (m)</span><span>Mem (Mi)</span>
    </div>
    <VirtualList items={rows} itemHeight={28}>
      {#snippet row(r: MetricsRow)}
        <div class="rtable-row" style="grid-template-columns: 1.5fr 2.5fr 1fr 1fr;">
          <span class="dim">{r.namespace ?? "—"}</span>
          <span>{r.name}</span>
          <span>{r.cpuMillicores}</span>
          <span>{r.memMib}</span>
        </div>
      {/snippet}
    </VirtualList>
    {#if !loading && rows.length === 0}
      <p class="dim pad">metrics-server not available or no data</p>
    {/if}
  {/if}
</div>
