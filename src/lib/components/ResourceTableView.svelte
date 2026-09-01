<script lang="ts">
  import { onDestroy, onMount } from "svelte";
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
  let loading = $state(true);
  let timer: ReturnType<typeof setInterval> | undefined;

  async function refresh() {
    try {
      const ns = resourceKind.namespaced ? session.namespace : null;
      const t = await api.listResourceTable(tabId, resourceKind.group, resourceKind.version, resourceKind.plural, ns);
      columns = t.columns;
      rows = t.rows;
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
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

  onMount(() => {
    timer = setInterval(refresh, 5000);
  });
  onDestroy(() => clearInterval(timer));

  // Reads session.namespace (via refresh) synchronously on each run, so this
  // fires once on mount and again whenever the tab's namespace changes.
  $effect(() => {
    refresh();
  });
</script>

<div class="rtable">
  {#if error}
    <div class="connect-error"><pre class="mono">{error}</pre></div>
  {:else}
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
