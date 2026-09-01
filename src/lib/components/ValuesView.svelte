<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ConfigEntry, type ResourceKind } from "../api";
  import type { TabSession } from "../stores/sessions.svelte";
  import { matchRow } from "../command";
  import { copyText } from "../clipboard";
  import { SvelteSet } from "svelte/reactivity";

  let {
    tabId,
    resourceKind,
    namespace,
    name,
    session,
  }: {
    tabId: number;
    resourceKind: ResourceKind;
    namespace: string;
    name: string;
    session: TabSession;
  } = $props();

  const secret = $derived(resourceKind.kind === "Secret");
  let entries = $state<ConfigEntry[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(true);
  // Secret values start masked; reveal is per key.
  const revealed = new SvelteSet<string>();
  let copiedKey = $state<string | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  const visible = $derived(entries.filter((e) => matchRow(e.key, session.filter)));

  function preview(e: ConfigEntry): string {
    if (secret && !revealed.has(e.key)) return "••••••••";
    const flat = e.value.replace(/\n/g, "⏎");
    return flat.length > 160 ? flat.slice(0, 160) + "…" : flat;
  }

  async function copy(e: ConfigEntry) {
    try {
      await copyText(e.value);
      copiedKey = e.key;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copiedKey = null), 1500);
    } catch (err) {
      error = String(err);
    }
  }

  onMount(async () => {
    try {
      entries = await api.getConfigValues(tabId, namespace, name, resourceKind.kind);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });
</script>

<div class="rtable">
  {#if error}
    <div class="connect-error"><pre class="mono">{error}</pre></div>
  {/if}
  <div class="rtable-head" style="grid-template-columns: 1.2fr 3fr 1fr;">
    <span>Key</span><span>Value</span><span></span>
  </div>
  {#each visible as e (e.key)}
    <div class="rtable-row" style="grid-template-columns: 1.2fr 3fr 1fr;">
      <span class="mono">{e.key}{#if e.binary}&nbsp;<span class="dim">(binary, base64)</span>{/if}</span>
      <span class="mono dim">{preview(e)}</span>
      <span class="values-actions">
        {#if secret}
          <button
            onclick={() => (revealed.has(e.key) ? revealed.delete(e.key) : revealed.add(e.key))}>
            {revealed.has(e.key) ? "Hide" : "Show"}
          </button>
        {/if}
        <button onclick={() => copy(e)}>{copiedKey === e.key ? "Copied!" : "Copy"}</button>
      </span>
    </div>
  {/each}
  {#if loading}
    <p class="dim pad">Loading…</p>
  {:else if entries.length === 0}
    <p class="dim pad">{resourceKind.kind} {namespace}/{name} has no data keys.</p>
  {/if}
</div>
