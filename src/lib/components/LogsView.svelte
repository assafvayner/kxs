<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { api } from "../api";
  import type { TabSession } from "../stores/sessions.svelte";
  import { matchRow } from "../command";
  import { copyText } from "../clipboard";
  import { SINCE_OPTIONS, defaultTail, logWindow, tailOptions } from "../logOptions";
  let {
    tabId,
    namespace,
    pods,
    label,
    session,
  }: {
    tabId: number;
    namespace: string;
    pods: string[];
    label: string;
    session: TabSession;
  } = $props();

  const multi = $derived(pods.length > 1);
  let containers = $state<string[]>([]);
  let container = $state<string | undefined>(undefined);
  let follow = $state(true);
  let previous = $state(false);
  let timestamps = $state(false);
  let sinceSeconds = $state(0);
  let tail = $state(untrack(() => defaultTail(pods.length > 1)));
  let wrap = $state(true);
  let copied = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  let lines = $state<string[]>([]);
  let error = $state<string | null>(null);
  let streamIds: number[] = [];
  let seq = 0;
  const CAP = 10000;

  let body = $state<HTMLElement | undefined>(undefined);
  // Sticky-bottom: keep following the tail while the user is at (or near) the
  // end; scrolling up detaches, scrolling back down re-attaches.
  let atBottom = true;
  function onBodyScroll() {
    if (!body) return;
    atBottom = body.scrollTop + body.clientHeight >= body.scrollHeight - 8;
  }

  function stopAll() {
    for (const id of streamIds) api.stopLogs(tabId, id).catch(() => {});
    streamIds = [];
  }

  async function start() {
    const mySeq = ++seq;
    stopAll();
    lines = [];
    atBottom = true;
    // Interleaved by arrival; each pod's own lines stay in order.
    await Promise.all(
      pods.map(async (pod) => {
        try {
          const id = await api.streamLogs(
            tabId,
            {
              namespace,
              pod,
              container: multi ? undefined : container,
              // previous logs are a finished stream, so they can never follow
              follow: follow && !previous,
              previous: !multi && previous,
              timestamps,
              ...logWindow(sinceSeconds, tail),
            },
            (ev) => {
              if (mySeq !== seq) return; // superseded stream: ignore late events
              if (ev.type === "lines") {
                const tagged = multi ? ev.lines.map((l) => `[${pod}] ${l}`) : ev.lines;
                lines = [...lines, ...tagged].slice(-CAP);
              } else if (ev.type === "error") {
                error = multi ? `${pod}: ${ev.message}` : ev.message;
              }
            },
          );
          if (mySeq !== seq) {
            // another start() superseded us while awaiting; stop the orphan
            api.stopLogs(tabId, id).catch(() => {});
            return;
          }
          streamIds.push(id);
        } catch (e) {
          if (mySeq === seq) error = String(e);
        }
      }),
    );
  }

  onMount(async () => {
    if (!multi) {
      try {
        containers = await api.listContainers(tabId, namespace, pods[0]);
        container = containers[0];
      } catch (e) {
        error = String(e);
      }
    }
    await start();
  });
  onDestroy(() => {
    stopAll();
    clearTimeout(copyTimer);
  });

  async function copyVisible() {
    try {
      await copyText(visible.join("\n"));
      copied = true;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copied = false), 1200);
    } catch (e) {
      error = String(e);
    }
  }

  const visible = $derived(session.filter ? lines.filter((l) => matchRow(l, session.filter)) : lines);

  // Keep the view pinned to the newest line while the user hasn't scrolled up.
  $effect(() => {
    if (visible.length === 0 || !body || !atBottom) return;
    requestAnimationFrame(() => {
      if (body && atBottom) body.scrollTop = body.scrollHeight;
    });
  });
</script>

<div class="detail">
  <div class="detail-bar">
    <span class="mono">{namespace}/{label}</span>
    {#if !multi}
      <select bind:value={container} onchange={start}>
        {#each containers as c}<option value={c}>{c}</option>{/each}
      </select>
    {/if}
    <select bind:value={sinceSeconds} onchange={start} title="since">
      {#each SINCE_OPTIONS as o}<option value={o.seconds}>{o.label}</option>{/each}
    </select>
    {#if sinceSeconds === 0}
      <select bind:value={tail} onchange={start} title="tail lines{multi ? ' per pod' : ''}">
        {#each tailOptions(multi) as n}<option value={n}>{n} lines</option>{/each}
      </select>
    {/if}
    <label class="chk">
      <input type="checkbox" bind:checked={follow} onchange={start} disabled={previous} /> follow
    </label>
    {#if !multi}
      <label class="chk"><input type="checkbox" bind:checked={previous} onchange={start} /> previous</label>
    {/if}
    <label class="chk"><input type="checkbox" bind:checked={timestamps} onchange={start} /> ts</label>
    <label class="chk"><input type="checkbox" bind:checked={wrap} /> wrap</label>
    <button onclick={copyVisible}>{copied ? "Copied!" : "Copy"}</button>
    <span class="dim">{visible.length}/{lines.length}</span>
  </div>
  {#if error}<div class="connect-error"><pre class="mono">{error}</pre></div>{/if}
  <pre
    bind:this={body}
    onscroll={onBodyScroll}
    class="detail-body logs mono"
    class:nowrap={!wrap}>{visible.join("\n")}</pre>
</div>
