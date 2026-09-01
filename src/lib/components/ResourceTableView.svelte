<script lang="ts">
  import { onDestroy } from "svelte";
  import { api, type ResourceKind, type ResourceRow, type ResourceTableEvent } from "../api";
  import type { TabSession } from "../stores/sessions.svelte";
  import { now } from "../stores/now.svelte";
  import { age } from "../age";
  import { matchRow, splitFilter } from "../command";
  import { cycleSort, sortIndicator, sortRows, type Sort } from "../sort";
  import { ColumnWidths, resourceTableId, startColumnDrag } from "../columnResize.svelte";
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
  /** Column index + direction, or null for the server's own row order. */
  let sort = $state<Sort<number> | null>(null);
  /** Label selector actually in effect; only a settled one restarts the watch. */
  let labelSelector = $state<string | null>(null);
  const LABEL_DEBOUNCE_MS = 400;

  function stop() {
    if (watchId !== null) {
      api.stopResourceTable(tabId, watchId).catch(() => {});
      watchId = null;
    }
  }

  // Live table: the backend re-renders the server-side Table on every
  // (debounced) change to this kind and pushes it through the channel.
  async function start(ns: string | null, selector: string | null) {
    const mySeq = ++seq;
    stop();
    loading = true;
    error = null;
    reconnecting = false;
    try {
      const onEvent = (ev: ResourceTableEvent) => {
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
      };
      const id = await api.watchResourceTable(tabId, resourceKind, ns, onEvent, selector);
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

  const filter = $derived(splitFilter(session.filter));
  // Sorted after filtering so j/k walks exactly what is on screen.
  const visible = $derived.by(() => {
    const matched = rows.filter((r) => matchRow(r.name, filter.name));
    return sort === null ? matched : sortRows(matched, sort.key, sort.dir);
  });
  const selectedIndex = $derived(
    session.selected === null ? -1 : visible.findIndex((r) => r.key === session.selected),
  );

  // Publish visible keys so keyboard selection (j/k) can walk this table.
  $effect(() => {
    session.visibleKeys = visible.map((r) => r.key);
  });

  onDestroy(stop);

  // A column index only means something for one kind's column set.
  $effect(() => {
    void resourceKind;
    sort = null;
  });

  const DEFAULT_TRACK = "minmax(80px, 1fr)";
  const columnWidths = new ColumnWidths();
  const template = $derived(columnWidths.template(columns.map(() => DEFAULT_TRACK)));

  // Rebinds on a kind switch and whenever the served column set changes size.
  $effect(() => {
    columnWidths.configure(resourceTableId(resourceKind), columns.length);
  });

  // Typing a selector must not spawn a watch per keystroke.
  $effect(() => {
    const next = filter.labels;
    if (next === labelSelector) return;
    const t = setTimeout(() => (labelSelector = next), LABEL_DEBOUNCE_MS);
    return () => clearTimeout(t);
  });

  // Reads session.namespace synchronously, so this fires once on mount and
  // again whenever the tab's namespace or label selector changes.
  $effect(() => {
    start(resourceKind.namespaced ? session.namespace : null, labelSelector);
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
      {#each columns as c, i}<div class="col-cell">
          <button type="button" class="col-head" onclick={() => (sort = cycleSort(sort, i))}
            >{c}{sortIndicator(sort, i)}</button
          ><button
            type="button"
            class="col-resizer"
            tabindex="-1"
            aria-label="Resize {c} column"
            onpointerdown={(e) =>
              startColumnDrag(e, {
                onwidth: (w) => columnWidths.set(i, w),
                oncommit: () => columnWidths.persist(),
              })}
            ondblclick={() => columnWidths.reset(i)}></button>
        </div>{/each}
    </div>
    <VirtualList items={visible} itemHeight={28} scrollToIndex={selectedIndex}>
      {#snippet row(r: ResourceRow)}
        <div
          class="rtable-row"
          class:selected={session.selected === r.key}
          style="grid-template-columns: {template};"
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
