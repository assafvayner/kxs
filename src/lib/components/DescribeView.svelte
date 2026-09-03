<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ResourceKind } from "../api";
  import type { TabSession } from "../stores/sessions.svelte";
  import HighlightedText from "./HighlightedText.svelte";
  let {
    tabId,
    title,
    resourceKind,
    namespace,
    name,
    session,
  }: {
    tabId: number;
    title: string;
    resourceKind: ResourceKind;
    namespace: string | null;
    name: string;
    session: TabSession;
  } = $props();
  let body = $state<string | null>(null);
  let error = $state<string | null>(null);
  onMount(async () => {
    try {
      body = await api.describeResource(tabId, resourceKind, namespace, name);
    } catch (e) {
      error = String(e);
    }
  });
</script>

<div class="detail">
  <div class="detail-bar"><span class="mono">{title}</span></div>
  <div class="detail-body">
    {#if error}<div class="error">{error}</div>
    {:else if body === null}<div class="dim">Loading…</div>
    {:else}<HighlightedText {body} query={session.filter} />{/if}
  </div>
</div>
