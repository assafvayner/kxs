<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, type ForwardInfo } from "../api";

  let { tabId }: { tabId: number } = $props();

  let forwards = $state<ForwardInfo[]>([]);
  let error = $state<string | null>(null);
  let timer: ReturnType<typeof setInterval> | undefined;

  async function refresh() {
    try {
      forwards = await api.listForwards(tabId);
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  async function stop(id: number) {
    try {
      await api.stopForward(tabId, id);
    } catch (e) {
      error = String(e);
    } finally {
      await refresh();
    }
  }

  onMount(() => {
    refresh();
    timer = setInterval(refresh, 2000);
  });
  onDestroy(() => clearInterval(timer));
</script>

<div class="rtable">
  {#if error}
    <div class="connect-error"><pre class="mono">{error}</pre></div>
  {/if}
  <div class="rtable-head" style="grid-template-columns: 2fr 1fr;">
    <span>Local address</span><span></span>
  </div>
  {#each forwards as fwd (fwd.id)}
    <div class="rtable-row" style="grid-template-columns: 2fr 1fr;">
      <span class="mono">127.0.0.1:{fwd.localPort}</span>
      <span><button onclick={() => stop(fwd.id)}>Stop</button></span>
    </div>
  {/each}
  {#if forwards.length === 0}
    <p class="dim pad">No active port-forwards. Select a pod and press f to start one.</p>
  {/if}
</div>
