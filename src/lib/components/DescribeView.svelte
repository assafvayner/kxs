<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ResourceEvent } from "../api";
  let {
    tabId,
    title,
    namespace,
    name,
    body,
  }: { tabId: number; title: string; namespace: string | null; name: string; body: string } = $props();
  let events = $state<ResourceEvent[]>([]);
  let eventsError = $state<string | null>(null);
  onMount(async () => {
    try {
      events = await api.getResourceEvents(tabId, namespace, name);
    } catch (e) {
      eventsError = String(e);
    }
  });
</script>

<div class="detail">
  <div class="detail-bar"><span class="mono">{title}</span></div>
  <div class="detail-body">
    <pre class="mono">{body}</pre>
    <h3 class="events-h">Events</h3>
    {#if eventsError}<div class="dim">events unavailable: {eventsError}</div>
    {:else if events.length === 0}<div class="dim">No events.</div>
    {:else}
      <table class="events">
        <thead><tr><th>Type</th><th>Reason</th><th>Count</th><th>Message</th></tr></thead>
        <tbody>
          {#each events as ev}
            <tr>
              <td class:st-bad={ev.type === "Warning"}>{ev.type}</td>
              <td>{ev.reason}</td><td>{ev.count}</td><td>{ev.message}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
</div>
