<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, type MetricsRow, type NodeMetricsRow } from "../api";
  import { matchRow } from "../command";
  import { ofTotal } from "../utilization";
  import type { TabSession } from "../stores/sessions.svelte";
  import VirtualList from "./VirtualList.svelte";

  let { tabId, session }: { tabId: number; session: TabSession } = $props();

  let rows = $state<MetricsRow[]>([]);
  let nodes = $state<NodeMetricsRow[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let timer: ReturnType<typeof setInterval> | undefined;
  const visible = $derived(rows.filter((r) => matchRow(r.name, session.filter)));

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
    // Node metrics are cluster-scoped; a failure here must not hide the pods.
    try {
      nodes = await api.nodeMetrics(tabId);
    } catch {
      nodes = [];
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

  const NODE_COLS = "grid-template-columns: 2.5fr 1.5fr 1.5fr;";
  const POD_COLS = "grid-template-columns: 1.5fr 2.5fr 1fr 1fr;";
</script>

<div class="rtable">
  {#if error}
    <div class="connect-error"><pre class="mono">{error}</pre></div>
  {:else}
    {#if nodes.length > 0}
      <div class="rtable-head" style={NODE_COLS}>
        <span>Node</span><span>CPU</span><span>Mem</span>
      </div>
      {#each nodes as n (n.name)}
        {@const cpu = ofTotal(n.cpuMillicores, n.cpuAllocatableMillicores, "m")}
        {@const mem = ofTotal(n.memMib, n.memAllocatableMib, "Mi")}
        <div class="rtable-row" style={NODE_COLS}>
          <span>{n.name}</span>
          <span class={cpu.cls}>{cpu.text}</span>
          <span class={mem.cls}>{mem.text}</span>
        </div>
      {/each}
    {/if}
    <div class="rtable-head" style={POD_COLS}>
      <span>Namespace</span><span>Pod</span><span>CPU (m)</span><span>Mem (Mi)</span>
    </div>
    <VirtualList items={visible} itemHeight={28}>
      {#snippet row(r: MetricsRow)}
        <div class="rtable-row" style={POD_COLS}>
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
