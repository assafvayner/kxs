<script lang="ts">
  import { onDestroy } from "svelte";
  import { api, type ResourceKind, type ResourceRow } from "../api";
  import type { TabSession } from "../stores/sessions.svelte";
  import { now } from "../stores/now.svelte";
  import { age } from "../age";
  import { matchRow } from "../command";
  import VirtualList from "./VirtualList.svelte";

  let {
    tabId,
    session,
    resourceKind,
    onactivate,
    oncontextmenu,
  }: {
    tabId: number;
    session: TabSession;
    resourceKind: ResourceKind;
    onactivate: (key: string) => void;
    oncontextmenu: (key: string, e: MouseEvent) => void;
  } = $props();

  let columns = $state<string[]>([]);
  let rows = $state<ResourceRow[]>([]);
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

  // Live table: the backend re-renders the server-side Table on every
  // (debounced) change to this kind and pushes it through the channel.
  async function start(ns: string | null) {
    const mySeq = ++seq;
    stop();
    loading = true;
    error = null;
    reconnecting = false;
    try {
      const id = await api.watchResourceTable(tabId, resourceKind, ns, (ev) => {
        if (mySeq !== seq) return; // superseded watch: ignore late events
        if (ev.type === "table") {
          columns = ev.table.columns;
          rows = ev.table.rows;
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

  const visible = $derived(rows.filter((r) => matchRow(r.name, session.filter)));
  const selectedIndex = $derived(
    session.selected === null ? -1 : visible.findIndex((r) => r.key === session.selected),
  );

  // Publish visible keys so keyboard selection (j/k) can walk this table.
  $effect(() => {
    session.visibleKeys = visible.map((r) => r.key);
  });

  onDestroy(stop);

  // Reads session.namespace synchronously, so this fires once on mount and
  // again whenever the tab's namespace changes, restarting the watch.
  $effect(() => {
    start(resourceKind.namespaced ? session.namespace : null);
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
    <div class="rtable-head" style="grid-template-columns: repeat({columns.length}, minmax(80px, 1fr));">
      {#each columns as c}<span>{c}</span>{/each}
    </div>
    <VirtualList items={visible} itemHeight={28} scrollToIndex={selectedIndex}>
      {#snippet row(r: ResourceRow)}
        <div
          class="rtable-row"
          class:selected={session.selected === r.key}
          style="grid-template-columns: repeat({columns.length}, minmax(80px, 1fr));"
          role="button"
          tabindex="0"
          onclick={() => (session.selected = r.key)}
          ondblclick={() => onactivate(r.key)}
          oncontextmenu={(e) => oncontextmenu(r.key, e)}
          onkeydown={(e) => {
            if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) {
              e.preventDefault();
              session.selected = r.key;
            }
          }}>
          {#each r.cells as cell}<span>{cell}</span>{/each}
          <span>{age(r.created, now.ms)}</span>
        </div>
      {/snippet}
    </VirtualList>
    {#if loading && rows.length === 0}<p class="dim pad">Loading…</p>{/if}
  {/if}
</div>
