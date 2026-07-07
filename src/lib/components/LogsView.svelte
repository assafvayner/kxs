<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api } from "../api";
  import type { TabSession } from "../stores/sessions.svelte";
  import { matchRow } from "../command";
  let { tabId, namespace, pod, session }: { tabId: number; namespace: string; pod: string; session: TabSession } = $props();

  let containers = $state<string[]>([]);
  let container = $state<string | undefined>(undefined);
  let follow = $state(true);
  let wrap = $state(true);
  let lines = $state<string[]>([]);
  let error = $state<string | null>(null);
  let streamId: number | undefined;
  let seq = 0;
  const CAP = 10000;

  let body = $state<HTMLElement | undefined>(undefined);
  let scrolledToEnd = false;

  async function start() {
    const mySeq = ++seq;
    if (streamId !== undefined) {
      api.stopLogs(tabId, streamId).catch(() => {});
      streamId = undefined;
    }
    lines = [];
    scrolledToEnd = false;
    try {
      const id = await api.streamLogs(
        tabId,
        { namespace, pod, container, follow, tailLines: 1000, timestamps: false },
        (ev) => {
          if (mySeq !== seq) return; // superseded stream: ignore late events
          if (ev.type === "lines") {
            lines = [...lines, ...ev.lines].slice(-CAP);
          } else if (ev.type === "error") {
            error = ev.message;
          }
        },
      );
      if (mySeq !== seq) {
        // another start() superseded us while awaiting; stop the orphan
        api.stopLogs(tabId, id).catch(() => {});
        return;
      }
      streamId = id;
    } catch (e) {
      if (mySeq === seq) error = String(e);
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

  const visible = $derived(session.filter ? lines.filter((l) => matchRow(l, session.filter)) : lines);

  // Jump to the newest log line once the first batch renders.
  $effect(() => {
    if (scrolledToEnd || visible.length === 0 || !body) return;
    scrolledToEnd = true;
    requestAnimationFrame(() => body && (body.scrollTop = body.scrollHeight));
  });
</script>

<div class="detail">
  <div class="detail-bar">
    <span class="mono">{namespace}/{pod}</span>
    <select bind:value={container} onchange={start}>
      {#each containers as c}<option value={c}>{c}</option>{/each}
    </select>
    <label class="chk"><input type="checkbox" bind:checked={follow} onchange={start} /> follow</label>
    <label class="chk"><input type="checkbox" bind:checked={wrap} /> wrap</label>
    <span class="dim">{visible.length}/{lines.length}</span>
  </div>
  {#if error}<div class="connect-error"><pre class="mono">{error}</pre></div>{/if}
  <pre bind:this={body} class="detail-body logs mono" class:nowrap={!wrap}>{visible.join("\n")}</pre>
</div>
