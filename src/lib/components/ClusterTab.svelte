<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api } from "../api";
  import type { PodRow, ResourceKind } from "../api";
  import { sessions, TabSession } from "../stores/sessions.svelte";
  import type { View } from "../stores/viewstack.svelte";
  import { now } from "../stores/now.svelte";
  import { age } from "../age";
  import { handleClusterKey, clusterKeyHandlers, type ClusterActions } from "../clusterKeys";
  import { currentKindLabel } from "../command";
  import VirtualList from "./VirtualList.svelte";
  import CommandBar from "./CommandBar.svelte";
  import ConfirmBar from "./ConfirmBar.svelte";
  import ResourcePicker from "./ResourcePicker.svelte";
  import ResourceTableView from "./ResourceTableView.svelte";
  import YamlView from "./YamlView.svelte";
  import DescribeView from "./DescribeView.svelte";
  import LogsView from "./LogsView.svelte";
  import TerminalView from "./TerminalView.svelte";
  import ForwardsView from "./ForwardsView.svelte";
  import MetricsView from "./MetricsView.svelte";

  let { context, tabId }: { context: string; tabId: number } = $props();

  const s = new TabSession();
  // svelte-ignore state_referenced_locally
  sessions.set(tabId, s);

  let destroyed = false;
  let bar = $state<"command" | "filter" | null>(null);
  let confirm = $state<null | {
    message: string;
    kind: "confirm" | "number";
    run: (value?: number) => Promise<void>;
    clearSelectionOnSuccess?: boolean;
  }>(null);
  let actionError = $state<string | null>(null);

  const SCALABLE_KINDS = ["Deployment", "StatefulSet", "ReplicaSet", "ReplicationController"];
  const RESTARTABLE_KINDS = ["Deployment", "StatefulSet", "DaemonSet"];

  async function connect() {
    s.status = "connecting";
    s.error = null;
    try {
      const info = await api.openSession(tabId, context);
      if (destroyed) {
        api.closeSession(tabId).catch(() => {});
        return;
      }
      s.version = info.version;
      s.namespace = info.defaultNamespace || null;
      s.status = "ready";
      api
        .listNamespaces(tabId)
        .then((ns) => (s.namespaces = ns.sort()))
        .catch(() => {});
      api
        .listResourceKinds(tabId)
        .then((k) => (s.kinds = k))
        .catch(() => {});
      await startWatch();
    } catch (e) {
      if (destroyed) return;
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

  const POD_KIND: ResourceKind = {
    group: "",
    version: "v1",
    kind: "Pod",
    plural: "pods",
    namespaced: true,
    aliases: ["po", "pod", "pods"],
  };

  function currentKind(): ResourceKind {
    const top = s.views.top;
    return top.kind === "resource" ? top.resourceKind : POD_KIND;
  }

  function parseSelected(): { namespace: string | null; name: string } | null {
    if (s.selected === null) return null;
    const i = s.selected.indexOf("/");
    if (i === -1) return { namespace: null, name: s.selected };
    return { namespace: s.selected.slice(0, i), name: s.selected.slice(i + 1) };
  }

  function pushView(v: View) {
    s.selected = null;
    s.views.push(v);
  }
  function popView() {
    s.selected = null;
    s.views.pop();
  }
  function popTo(i: number) {
    while (s.views.depth > i + 1) s.views.pop();
    s.selected = null;
  }

  function viewLabel(v: View): string {
    switch (v.kind) {
      case "pods":
        return "pods";
      case "resource":
        return v.resourceKind.kind;
      case "yaml":
      case "describe":
        return v.title;
      case "logs":
        return `logs: ${v.pod}`;
      case "exec":
        return `exec: ${v.pod}`;
      case "forwards":
        return "port-forwards";
      case "metrics":
        return "metrics";
    }
  }

  async function openDetail(kind: "yaml" | "describe") {
    const sel = parseSelected();
    if (!sel) return;
    const k = currentKind();
    try {
      const body = await api.getResourceYaml(tabId, k, sel.namespace, sel.name);
      if (kind === "yaml") {
        pushView({ kind: "yaml", title: `${k.kind} ${sel.name}`, body });
      } else {
        pushView({
          kind: "describe",
          title: `${k.kind} ${sel.name}`,
          namespace: sel.namespace,
          name: sel.name,
          body,
        });
      }
    } catch {
      /* selection may be stale; ignore */
    }
  }

  const actions: ClusterActions = {
    openCommand: () => (bar = "command"),
    openFilter: () => (bar = "filter"),
    back: () => {
      if (bar) bar = null;
      else popView();
    },
    describe: () => openDetail("describe"),
    yaml: () => openDetail("yaml"),
    logs: () => {
      const k = currentKind();
      if (k.kind !== "Pod" || k.group !== "") return;
      const sel = parseSelected();
      if (!sel || sel.namespace === null) return;
      pushView({ kind: "logs", namespace: sel.namespace, pod: sel.name });
    },
    enter: () => openDetail("describe"),
    del: () => {
      const sel = parseSelected();
      if (!sel) return;
      const k = currentKind();
      confirm = {
        message: `Delete ${k.kind} ${sel.name}?`,
        kind: "confirm",
        run: () => api.deleteResource(tabId, k, sel.namespace, sel.name),
        clearSelectionOnSuccess: true,
      };
    },
    scale: () => {
      const sel = parseSelected();
      if (!sel) return;
      const k = currentKind();
      if (!SCALABLE_KINDS.includes(k.kind)) {
        actionError = `scale not supported for ${k.kind}`;
        return;
      }
      confirm = {
        message: `Scale ${sel.name} to`,
        kind: "number",
        run: (n) => api.scaleResource(tabId, k, sel.namespace, sel.name, n ?? 1),
      };
    },
    restart: () => {
      const sel = parseSelected();
      if (!sel) return;
      const k = currentKind();
      if (!RESTARTABLE_KINDS.includes(k.kind)) {
        actionError = `restart not supported for ${k.kind}`;
        return;
      }
      confirm = {
        message: `Restart rollout of ${sel.name}?`,
        kind: "confirm",
        run: () =>
          api.restartResource(tabId, k, sel.namespace, sel.name, new Date().toISOString()),
      };
    },
    cordon: () => {
      const sel = parseSelected();
      if (!sel) return;
      const k = currentKind();
      if (k.kind !== "Node" || k.group !== "") {
        actionError = `cordon not supported for ${k.kind}`;
        return;
      }
      confirm = {
        message: `Cordon ${sel.name}?`,
        kind: "confirm",
        run: () => api.cordonNode(tabId, sel.name, true),
      };
    },
    shell: () => {
      const k = currentKind();
      if (k.kind !== "Pod" || k.group !== "") {
        actionError = `shell not supported for ${k.kind}`;
        return;
      }
      const sel = parseSelected();
      if (!sel || sel.namespace === null) return;
      pushView({ kind: "exec", namespace: sel.namespace, pod: sel.name, container: null });
    },
    forward: () => {
      const k = currentKind();
      if (k.kind !== "Pod" || k.group !== "") {
        actionError = `port-forward not supported for ${k.kind}`;
        return;
      }
      const sel = parseSelected();
      if (!sel || sel.namespace === null) return;
      confirm = {
        message: "Forward pod port",
        kind: "number",
        run: async (port) => {
          await api.startForward(tabId, sel.namespace!, sel.name, port ?? 8080);
          pushView({ kind: "forwards" });
        },
      };
    },
    hasSelection: () => s.selected !== null,
  };

  async function onConfirmAccept(value?: number) {
    if (!confirm) return;
    const run = confirm.run;
    const clearSelectionOnSuccess = confirm.clearSelectionOnSuccess === true;
    try {
      await run(value);
      actionError = null;
      if (clearSelectionOnSuccess) s.selected = null;
    } catch (e) {
      actionError = String(e);
    } finally {
      confirm = null;
    }
  }
  function onConfirmCancel() {
    confirm = null;
  }

  onMount(() => {
    clusterKeyHandlers.set(tabId, (e) => {
      if (bar !== null || confirm !== null) return true; // a bar owns the keyboard; swallow
      return handleClusterKey(e, actions);
    });
    connect();
  });
  onDestroy(() => {
    destroyed = true;
    clusterKeyHandlers.delete(tabId);
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
      <ResourcePicker
        session={s}
        label={currentKindLabel(s.views.stack)}
        onpick={(k) => pushView({ kind: "resource", resourceKind: k })} />
      <nav class="breadcrumb">
        {#each s.views.stack as v, i (i)}
          {#if i > 0}<span class="sep">/</span>{/if}
          <button type="button" onclick={() => popTo(i)}>{viewLabel(v)}</button>
        {/each}
      </nav>
      <span class="spacer"></span>
      {#if s.views.top.kind === "pods"}
        <span class="dim">
          {s.pods.rows.length} pods
          {#if s.watchState === "reconnecting"}· <span class="st-warn">reconnecting…</span>{/if}
          {#if s.watchState === "starting"}· loading…{/if}
        </span>
      {/if}
    </div>

    {#if s.views.top.kind === "pods"}
      <div class="pod-table">
        <div class="pod-row pod-head">
          <span>NAMESPACE</span><span>NAME</span><span>READY</span><span>STATUS</span>
          <span>RESTARTS</span><span>IP</span><span>NODE</span><span>AGE</span>
        </div>
        <VirtualList items={s.pods.rows} itemHeight={28}>
          {#snippet row(pod: PodRow)}
            <div
              class="pod-row"
              class:selected={s.selected === pod.key}
              role="button"
              tabindex="0"
              onclick={() => (s.selected = pod.key)}
              onkeydown={(e) => {
                if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) {
                  e.preventDefault();
                  s.selected = pod.key;
                }
              }}>
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
    {:else if s.views.top.kind === "resource"}
      <ResourceTableView {tabId} session={s} resourceKind={s.views.top.resourceKind} />
    {:else if s.views.top.kind === "yaml"}
      <YamlView title={s.views.top.title} body={s.views.top.body} session={s} />
    {:else if s.views.top.kind === "describe"}
      <DescribeView
        {tabId}
        title={s.views.top.title}
        namespace={s.views.top.namespace}
        name={s.views.top.name}
        body={s.views.top.body}
        session={s} />
    {:else if s.views.top.kind === "logs"}
      <LogsView {tabId} namespace={s.views.top.namespace} pod={s.views.top.pod} session={s} />
    {:else if s.views.top.kind === "exec"}
      <TerminalView
        {tabId}
        namespace={s.views.top.namespace}
        pod={s.views.top.pod}
        container={s.views.top.container} />
    {:else if s.views.top.kind === "forwards"}
      <ForwardsView {tabId} session={s} />
    {:else if s.views.top.kind === "metrics"}
      <MetricsView {tabId} session={s} />
    {/if}

    {#if actionError}
      <div class="detail-bar">
        <span class="st-bad">{actionError}</span>
        <button onclick={() => (actionError = null)}>Dismiss</button>
      </div>
    {/if}

    {#if bar !== null}
      <CommandBar
        session={s}
        mode={bar}
        onclose={() => (bar = null)}
        onpick={(k) => pushView({ kind: "resource", resourceKind: k })}
        appCommands={{
          pf: () => pushView({ kind: "forwards" }),
          forwards: () => pushView({ kind: "forwards" }),
          top: () => pushView({ kind: "metrics" }),
          metrics: () => pushView({ kind: "metrics" }),
        }} />
    {/if}

    {#if confirm !== null}
      <ConfirmBar
        message={confirm.message}
        kind={confirm.kind}
        onconfirm={onConfirmAccept}
        oncancel={onConfirmCancel} />
    {/if}
  {/if}
</div>
