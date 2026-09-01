<script lang="ts">
  import { onDestroy } from "svelte";
  import { api, type ResourceKind, type ResourceRow } from "../api";
  import type { TabSession } from "../stores/sessions.svelte";
  import { now } from "../stores/now.svelte";
  import { age } from "../age";
  import { matchRow } from "../command";
  import {
    columnIndex,
    eventFilterText,
    eventTypeClass,
    sortEventsNewestFirst,
  } from "../events";
  import VirtualList from "./VirtualList.svelte";

  let { tabId, session }: { tabId: number; session: TabSession } = $props();

  const EVENT_KIND: ResourceKind = {
    group: "",
    version: "v1",
    kind: "Event",
    plural: "events",
    namespaced: true,
    aliases: ["ev", "event", "events"],
  };

  let columns = $state<string[]>([]);
  let rows = $state<ResourceRow[]>([]);
  let allNamespaces = $state(false);
  let error = $state<string | null>(null);
  let reconnecting = $state(false);
  let loading = $state(true);
  let watchId: number | null = null;
  let seq = 0;

  function stop() {
    if (watchId !== null) {
      api.stopResourceTable(tabId, watchId).catch(() => {});
      watchId = null;
    }
  }

  async function start(ns: string | null) {
    const mySeq = ++seq;
    stop();
    loading = true;
    error = null;
    reconnecting = false;
    allNamespaces = ns === null;
    try {
      const id = await api.watchResourceTable(tabId, EVENT_KIND, ns, (ev) => {
        if (mySeq !== seq) return; // superseded watch: ignore late events
        if (ev.type === "table") {
          columns = ev.table.columns;
          // The API server returns events unordered; anchor the Last Seen
          // fallback to the arrival time so the order doesn't churn per tick.
          rows = sortEventsNewestFirst(
            ev.table.rows,
            columnIndex(ev.table.columns, "last seen"),
            Date.now(),
          );
          error = null;
          loading = false;
        } else if (ev.state === "error") {
          error = ev.message ?? "watch failed";
          loading = false;
          reconnecting = false;
        } else {
          reconnecting = ev.state === "reconnecting";
        }
      });
      if (mySeq !== seq) {
        // another start() superseded us while awaiting; stop the orphan
        api.stopResourceTable(tabId, id).catch(() => {});
        return;
      }
      watchId = id;
    } catch (e) {
      if (mySeq === seq) {
        error = String(e);
        loading = false;
      }
    }
  }

  const typeIndex = $derived(columnIndex(columns, "type"));
  const filterIndices = $derived([
    columnIndex(columns, "reason"),
    columnIndex(columns, "object"),
    columnIndex(columns, "message"),
  ]);
  const visible = $derived(
    rows.filter((r) => matchRow(eventFilterText(r, filterIndices), session.filter)),
  );

  const headColumns = $derived(allNamespaces ? ["Namespace", ...columns] : columns);
  const template = $derived(
    headColumns
      .map((c) => {
        const n = c.trim().toLowerCase();
        if (n === "message") return "minmax(220px, 4fr)";
        if (n === "object") return "minmax(140px, 2fr)";
        return "minmax(70px, 1fr)";
      })
      .join(" "),
  );

  onDestroy(stop);

  // Reads session.namespace synchronously, so this fires once on mount and
  // again whenever the tab's namespace changes, restarting the watch.
  $effect(() => {
    start(session.namespace);
  });
</script>

<div class="rtable">
  {#if error && rows.length === 0}
    <div class="connect-error"><pre class="mono">{error}</pre></div>
  {:else}
    {#if error}
      <p class="dim pad">{error}</p>
    {:else if reconnecting}
      <p class="dim pad">Reconnecting…</p>
    {/if}
    <div class="rtable-head" style="grid-template-columns: {template};">
      {#each headColumns as c}<span>{c}</span>{/each}
    </div>
    <VirtualList items={visible} itemHeight={28}>
      {#snippet row(r: ResourceRow)}
        <div class="rtable-row readonly" style="grid-template-columns: {template};">
          {#if allNamespaces}<span class="dim">{r.namespace ?? "—"}</span>{/if}
          {#each r.cells as cell, i}
            <span class={i === typeIndex ? eventTypeClass(cell) : undefined}>{cell}</span>
          {/each}
          <span>{age(r.created, now.ms)}</span>
        </div>
      {/snippet}
    </VirtualList>
    {#if loading && rows.length === 0}
      <p class="dim pad">Loading…</p>
    {:else if !loading && rows.length === 0}
      <p class="dim pad">no events</p>
    {/if}
  {/if}
</div>
