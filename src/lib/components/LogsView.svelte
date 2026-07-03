<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api } from "../api";
  let { tabId, namespace, pod }: { tabId: number; namespace: string; pod: string } = $props();

  let containers = $state<string[]>([]);
  let container = $state<string | undefined>(undefined);
  let follow = $state(true);
  let wrap = $state(true);
  let query = $state("");
  let lines = $state<string[]>([]);
  let error = $state<string | null>(null);
  let streamId: number | undefined;
  const CAP = 10000;

  async function start() {
    if (streamId !== undefined) {
      api.stopLogs(tabId, streamId).catch(() => {});
      streamId = undefined;
    }
    lines = [];
    try {
      streamId = await api.streamLogs(
        tabId,
        { namespace, pod, container, follow, tailLines: 1000, timestamps: false },
        (ev) => {
          if (ev.type === "lines") {
            lines = [...lines, ...ev.lines].slice(-CAP);
          } else if (ev.type === "error") {
            error = ev.message;
          }
        },
      );
    } catch (e) {
      error = String(e);
    }
  }

  onMount(async () => {
    try {
      containers = await api.listContainers(tabId, namespace, pod);
      container = containers[0];
    } catch (e) {
      error = String(e);
    }
    await start();
  });
  onDestroy(() => {
    if (streamId !== undefined) api.stopLogs(tabId, streamId).catch(() => {});
  });

  const visible = $derived(query ? lines.filter((l) => l.toLowerCase().includes(query.toLowerCase())) : lines);
</script>

<div class="detail">
  <div class="detail-bar">
    <span class="mono">{namespace}/{pod}</span>
    <select bind:value={container} onchange={start}>
      {#each containers as c}<option value={c}>{c}</option>{/each}
    </select>
    <label class="chk"><input type="checkbox" bind:checked={follow} onchange={start} /> follow</label>
    <label class="chk"><input type="checkbox" bind:checked={wrap} /> wrap</label>
    <input class="mono" placeholder="/ search" bind:value={query} />
    <span class="dim">{visible.length}/{lines.length}</span>
  </div>
  {#if error}<div class="connect-error"><pre class="mono">{error}</pre></div>{/if}
  <pre class="detail-body logs mono" class:nowrap={!wrap}>{visible.join("\n")}</pre>
</div>
