<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api } from "../api";
  import { sessions, TabSession } from "../stores/sessions.svelte";
  import { now } from "../stores/now.svelte";
  import { age } from "../age";
  import VirtualList from "./VirtualList.svelte";
  import type { PodRow } from "../api";

  let { context, tabId }: { context: string; tabId: number } = $props();

  const s = new TabSession();
  sessions.set(tabId, s);

  async function connect() {
    s.status = "connecting";
    s.error = null;
    try {
      const info = await api.openSession(tabId, context);
      s.version = info.version;
      s.namespace = info.defaultNamespace || null;
      s.status = "ready";
      api
        .listNamespaces(tabId)
        .then((ns) => (s.namespaces = ns.sort()))
        .catch(() => {});
      await startWatch();
    } catch (e) {
      s.status = "error";
      s.error = String(e);
    }
  }

  async function startWatch() {
    s.watchState = "starting";
    s.pods.apply({ type: "snapshot", rows: [] });
    try {
      await api.watchPods(tabId, s.namespace, (ev) => {
        if (ev.type === "status") {
          s.watchState = ev.state === "reconnecting" ? "reconnecting" : "live";
        } else {
          s.pods.apply(ev);
        }
      });
    } catch (e) {
      s.status = "error";
      s.error = String(e);
    }
  }

  function onNamespaceChange() {
    startWatch();
  }

  onMount(connect);
  onDestroy(() => {
    api.closeSession(tabId).catch(() => {});
    sessions.delete(tabId);
  });

  function statusClass(status: string): string {
    if (status === "Running" || status === "Completed") return "st-ok";
    if (status === "Pending" || status === "ContainerCreating" || status.startsWith("Init:"))
      return "st-warn";
    return "st-bad";
  }
</script>

<div class="cluster-tab">
  {#if s.status === "error"}
    <div class="connect-error">
      <h2>Cannot connect to {context}</h2>
      <pre class="mono">{s.error}</pre>
      <p class="dim">
        If this is an expired SSO/exec credential, refresh it in a terminal and retry.
      </p>
      <button class="primary" onclick={connect}>Retry</button>
    </div>
  {:else if s.status === "connecting"}
    <div class="connect-pending"><p class="dim">Connecting to {context}…</p></div>
  {:else}
    <div class="toolbar">
      <span class="ctx-name">{context}</span>
      <span class="dim mono">{s.version}</span>
      <label class="ns">
        namespace
        <select bind:value={s.namespace} onchange={onNamespaceChange}>
          <option value={null}>all</option>
          {#each s.namespaces as ns (ns)}
            <option value={ns}>{ns}</option>
          {/each}
        </select>
      </label>
      <span class="spacer"></span>
      <span class="dim">
        {s.pods.rows.length} pods
        {#if s.watchState === "reconnecting"}· <span class="st-warn">reconnecting…</span>{/if}
        {#if s.watchState === "starting"}· loading…{/if}
      </span>
    </div>
    <div class="pod-table">
      <div class="pod-row pod-head">
        <span>NAMESPACE</span><span>NAME</span><span>READY</span><span>STATUS</span>
        <span>RESTARTS</span><span>IP</span><span>NODE</span><span>AGE</span>
      </div>
      <VirtualList items={s.pods.rows} itemHeight={28}>
        {#snippet row(pod: PodRow)}
          <div class="pod-row">
            <span class="dim">{pod.namespace}</span>
            <span>{pod.name}</span>
            <span>{pod.ready}</span>
            <span class={statusClass(pod.status)}>{pod.status}</span>
            <span>{pod.restarts}</span>
            <span class="mono dim">{pod.ip ?? "—"}</span>
            <span class="dim">{pod.node ?? "—"}</span>
            <span>{age(pod.created, now.ms)}</span>
          </div>
        {/snippet}
      </VirtualList>
    </div>
  {/if}
</div>
