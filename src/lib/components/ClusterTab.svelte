<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api } from "../api";
  import type { DeleteOptions, PodRow, ResourceKind } from "../api";
  import { copyText } from "../clipboard";
  import { sessions, TabSession } from "../stores/sessions.svelte";
  import type { View } from "../stores/viewstack.svelte";
  import { now } from "../stores/now.svelte";
  import { age } from "../age";
  import {
    handleClusterKey,
    clusterKeyHandlers,
    moveSelection,
    type ClusterActions,
  } from "../clusterKeys";
  import { currentKindLabel, searchEnabled, matchRow } from "../command";
  import {
    menuItemsFor,
    SCALABLE_KINDS,
    RESTARTABLE_KINDS,
    POD_OWNER_KINDS,
    VIEW_PODS_KINDS,
    type MenuActionId,
    type MenuItem,
  } from "../contextMenu";
  import VirtualList from "./VirtualList.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import CommandBar from "./CommandBar.svelte";
  import ConfirmBar from "./ConfirmBar.svelte";
  import ResourcePicker from "./ResourcePicker.svelte";
  import SearchBar from "./SearchBar.svelte";
  import ResourceTableView from "./ResourceTableView.svelte";
  import YamlView from "./YamlView.svelte";
  import YamlEditView from "./YamlEditView.svelte";
  import DescribeView from "./DescribeView.svelte";
  import LogsView from "./LogsView.svelte";
  import TerminalView from "./TerminalView.svelte";
  import ForwardsView from "./ForwardsView.svelte";
  import MetricsView from "./MetricsView.svelte";
  import RolloutView from "./RolloutView.svelte";
  import ValuesView from "./ValuesView.svelte";

  let { context, tabId }: { context: string; tabId: number } = $props();

  const s = new TabSession();
  // svelte-ignore state_referenced_locally
  sessions.set(tabId, s);

  let destroyed = false;
  let bar = $state<"command" | null>(null);
  let confirm = $state<null | {
    message: string;
    kind: "confirm" | "number" | "delete";
    run: (value?: number, opts?: DeleteOptions) => Promise<void>;
    clearSelectionOnSuccess?: boolean;
  }>(null);
  let searchBar: { focus: () => void } | undefined = $state();
  let actionError = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let menu = $state<null | { x: number; y: number; groups: MenuItem[][] }>(null);

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
        .then((k) => {
          s.kinds = k;
          refreshPresentKinds();
        })
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

  async function refreshPresentKinds() {
    const kinds = s.kinds;
    if (!kinds.length) return;
    try {
      const keys = await api.listPresentKinds(tabId, s.namespace, kinds);
      s.presentKinds = new Set(keys);
    } catch {
      // leave the existing set (or null) in place → picker keeps showing the prior/all kinds
    }
  }

  function onNamespaceChange() {
    startWatch();
    refreshPresentKinds();
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

  const podsVisible = $derived(s.pods.rows.filter((p) => matchRow(p.name, s.filter)));
  const podsSelectedIndex = $derived(
    s.selected === null ? -1 : podsVisible.findIndex((p) => p.key === s.selected),
  );

  // The pods table lives in this component, so it publishes its visible keys
  // here; ResourceTableView does the same for resource views.
  $effect(() => {
    if (s.views.top.kind === "pods") s.visibleKeys = podsVisible.map((p) => p.key);
  });

  function parseSelected(): { namespace: string | null; name: string } | null {
    if (s.selected === null) return null;
    const i = s.selected.indexOf("/");
    if (i === -1) return { namespace: null, name: s.selected };
    return { namespace: s.selected.slice(0, i), name: s.selected.slice(i + 1) };
  }

  function pushView(v: View) {
    s.selected = null;
    menu = null;
    s.views.push(v);
  }
  function popView() {
    s.selected = null;
    menu = null;
    s.views.pop();
  }
  function popTo(i: number) {
    while (s.views.depth > i + 1) s.views.pop();
    s.selected = null;
    menu = null;
  }

  function viewLabel(v: View): string {
    switch (v.kind) {
      case "pods":
        return "pods";
      case "resource":
        return v.resourceKind.kind;
      case "yaml":
      case "yamlEdit":
      case "describe":
        return v.title;
      case "logs":
        return `logs: ${v.label}`;
      case "exec":
        return `exec: ${v.pod}`;
      case "forwards":
        return "port-forwards";
      case "metrics":
        return "metrics";
      case "rollout":
        return `rollout: ${v.name}`;
      case "values":
        return v.title;
    }
  }

  function activate(key: string) {
    s.selected = key;
    openDetail("yaml");
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
          resourceKind: k,
          namespace: sel.namespace,
          name: sel.name,
          body,
        });
      }
    } catch {
      /* selection may be stale; ignore */
    }
  }

  async function openEdit() {
    const sel = parseSelected();
    if (!sel) return;
    const k = currentKind();
    try {
      const body = await api.getResourceYaml(tabId, k, sel.namespace, sel.name);
      pushView({
        kind: "yamlEdit",
        title: `${k.kind} ${sel.name}`,
        body,
        resourceKind: k,
        namespace: sel.namespace,
        name: sel.name,
      });
    } catch {
      /* selection may be stale; ignore */
    }
  }

  function openRowMenu(key: string, e: MouseEvent) {
    e.preventDefault();
    s.selected = key;
    menu = { x: e.clientX, y: e.clientY, groups: menuItemsFor(currentKind()) };
  }

  function onMenuPick(id: MenuActionId) {
    menu = null;
    actions[id]();
  }

  function setCronjobSuspend(suspend: boolean) {
    const k = currentKind();
    const sel = parseSelected();
    if (!sel || sel.namespace === null) return;
    if (k.kind !== "CronJob") {
      actionError = `${suspend ? "suspend" : "resume"} not supported for ${k.kind}`;
      return;
    }
    confirm = {
      message: `${suspend ? "Suspend" : "Resume"} ${sel.name}?`,
      kind: "confirm",
      run: async () => {
        await api.suspendCronjob(tabId, sel.namespace!, sel.name, suspend);
        notice = `${suspend ? "suspended" : "resumed"} CronJob ${sel.name}`;
      },
    };
  }

  function requestRolloutUndo(revision: number) {
    const top = s.views.top;
    if (top.kind !== "rollout") return;
    const { namespace, name } = top;
    confirm = {
      message: `Roll back ${name} to revision ${revision}?`,
      kind: "confirm",
      run: async () => {
        await api.rolloutUndo(tabId, namespace, name, revision);
        notice = `rolled ${name} back to revision ${revision}`;
      },
    };
  }

  const actions: ClusterActions = {
    openCommand: () => (bar = "command"),
    focusSearch: () => searchBar?.focus(),
    back: () => {
      if (bar) bar = null;
      else popView();
    },
    describe: () => openDetail("describe"),
    yaml: () => openDetail("yaml"),
    edit: () => openEdit(),
    logs: () => {
      const k = currentKind();
      if (POD_OWNER_KINDS.includes(k.kind)) {
        actions.logsAll();
        return;
      }
      if (k.kind !== "Pod" || k.group !== "") return;
      const sel = parseSelected();
      if (!sel || sel.namespace === null) return;
      pushView({ kind: "logs", namespace: sel.namespace, pods: [sel.name], label: sel.name });
    },
    logsAll: async () => {
      const k = currentKind();
      const sel = parseSelected();
      if (!sel || sel.namespace === null) return;
      if (!POD_OWNER_KINDS.includes(k.kind)) {
        actionError = `logs not supported for ${k.kind}`;
        return;
      }
      const namespace = sel.namespace;
      try {
        const pods = await api.listWorkloadPods(tabId, k, namespace, sel.name);
        if (pods.length === 0) {
          actionError = `no pods match ${k.kind} ${sel.name}'s selector`;
          return;
        }
        pushView({ kind: "logs", namespace, pods, label: `${sel.name} (${pods.length} pods)` });
      } catch (e) {
        actionError = String(e);
      }
    },
    enter: () => openDetail("describe"),
    del: () => {
      const sel = parseSelected();
      if (!sel) return;
      const k = currentKind();
      confirm = {
        message: `Delete ${k.kind} ${sel.name}?`,
        kind: "delete",
        run: (_value, opts) => api.deleteResource(tabId, k, sel.namespace, sel.name, opts),
        clearSelectionOnSuccess: true,
      };
    },
    copyName: async () => {
      const sel = parseSelected();
      if (!sel) return;
      try {
        await copyText(sel.name);
        notice = `copied ${sel.name}`;
      } catch (e) {
        actionError = String(e);
      }
    },
    copyYaml: async () => {
      const sel = parseSelected();
      if (!sel) return;
      const k = currentKind();
      try {
        const body = await api.getResourceYaml(tabId, k, sel.namespace, sel.name);
        await copyText(body);
        notice = `copied YAML of ${k.kind} ${sel.name}`;
      } catch (e) {
        actionError = String(e);
      }
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
    uncordon: () => {
      const sel = parseSelected();
      if (!sel) return;
      const k = currentKind();
      if (k.kind !== "Node" || k.group !== "") {
        actionError = `uncordon not supported for ${k.kind}`;
        return;
      }
      confirm = {
        message: `Uncordon ${sel.name}?`,
        kind: "confirm",
        run: () => api.cordonNode(tabId, sel.name, false),
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
      const sel = parseSelected();
      if (!sel || sel.namespace === null) return;
      if (k.kind === "Pod" && k.group === "") {
        confirm = {
          message: "Forward pod port",
          kind: "number",
          run: async (port) => {
            await api.startForward(tabId, sel.namespace!, sel.name, port ?? 8080);
            pushView({ kind: "forwards" });
          },
        };
      } else if (k.kind === "Service" && k.group === "") {
        confirm = {
          message: `Forward service port of ${sel.name}`,
          kind: "number",
          run: async (port) => {
            await api.forwardService(tabId, sel.namespace!, sel.name, port ?? 80);
            pushView({ kind: "forwards" });
          },
        };
      } else {
        actionError = `port-forward not supported for ${k.kind}`;
      }
    },
    history: () => {
      const k = currentKind();
      const sel = parseSelected();
      if (!sel || sel.namespace === null) return;
      if (k.kind !== "Deployment") {
        actionError = `rollout history not supported for ${k.kind}`;
        return;
      }
      pushView({ kind: "rollout", namespace: sel.namespace, name: sel.name });
    },
    drain: () => {
      const sel = parseSelected();
      if (!sel) return;
      const k = currentKind();
      if (k.kind !== "Node" || k.group !== "") {
        actionError = `drain not supported for ${k.kind}`;
        return;
      }
      confirm = {
        message: `Drain ${sel.name}? (cordons, then evicts all non-DaemonSet pods)`,
        kind: "confirm",
        run: async () => {
          const r = await api.drainNode(tabId, sel.name);
          const failed = r.failed.length ? `, failed: ${r.failed.join("; ")}` : "";
          notice = `drained ${sel.name}: ${r.evicted} evicted, ${r.skipped} skipped${failed}`;
        },
      };
    },
    trigger: () => {
      const k = currentKind();
      const sel = parseSelected();
      if (!sel || sel.namespace === null) return;
      if (k.kind !== "CronJob") {
        actionError = `trigger not supported for ${k.kind}`;
        return;
      }
      confirm = {
        message: `Trigger ${sel.name} now?`,
        kind: "confirm",
        run: async () => {
          const job = await api.triggerCronjob(tabId, sel.namespace!, sel.name);
          notice = `created Job ${sel.namespace}/${job}`;
        },
      };
    },
    suspend: () => setCronjobSuspend(true),
    resume: () => setCronjobSuspend(false),
    viewPods: () => {
      const sel = parseSelected();
      if (!sel) return;
      const k = currentKind();
      if (!VIEW_PODS_KINDS.includes(k.kind)) {
        actionError = `view pods not supported for ${k.kind}`;
        return;
      }
      const name = sel.name;
      popTo(0);
      s.filter = name;
    },
    values: () => {
      const k = currentKind();
      const sel = parseSelected();
      if (!sel || sel.namespace === null) return;
      if (k.group !== "" || (k.kind !== "Secret" && k.kind !== "ConfigMap")) {
        actionError = `values not supported for ${k.kind}`;
        return;
      }
      pushView({
        kind: "values",
        title: `${k.kind} ${sel.name}`,
        resourceKind: k,
        namespace: sel.namespace,
        name: sel.name,
      });
    },
    move: (delta) => {
      const top = s.views.top.kind;
      if (top !== "pods" && top !== "resource") return false;
      const next = moveSelection(s.visibleKeys, s.selected, delta);
      if (next === null) return false;
      s.selected = next;
      return true;
    },
    hasSelection: () => s.selected !== null,
  };

  async function onConfirmAccept(value?: number, opts?: DeleteOptions) {
    if (!confirm) return;
    const run = confirm.run;
    const clearSelectionOnSuccess = confirm.clearSelectionOnSuccess === true;
    try {
      await run(value, opts);
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
      if (menu !== null) {
        // Any key dismisses the menu; Escape only dismisses, other keys fall
        // through so row shortcuts (l, x, s, …) still fire.
        menu = null;
        if (e.key === "Escape") {
          e.preventDefault();
          return true;
        }
      }
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
      <SearchBar bind:this={searchBar} session={s} enabled={searchEnabled(s.views.top)} />
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
        <VirtualList items={podsVisible} itemHeight={28} scrollToIndex={podsSelectedIndex}>
          {#snippet row(pod: PodRow)}
            <div
              class="pod-row"
              class:selected={s.selected === pod.key}
              role="button"
              tabindex="0"
              onclick={() => (s.selected = pod.key)}
              ondblclick={() => activate(pod.key)}
              oncontextmenu={(e) => openRowMenu(pod.key, e)}
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
      <ResourceTableView
        {tabId}
        session={s}
        resourceKind={s.views.top.resourceKind}
        onactivate={activate}
        oncontextmenu={openRowMenu} />
    {:else if s.views.top.kind === "yaml"}
      <YamlView title={s.views.top.title} body={s.views.top.body} session={s} />
    {:else if s.views.top.kind === "yamlEdit"}
      <YamlEditView
        {tabId}
        title={s.views.top.title}
        body={s.views.top.body}
        resourceKind={s.views.top.resourceKind}
        namespace={s.views.top.namespace}
        name={s.views.top.name}
        onClose={() => popView()} />
    {:else if s.views.top.kind === "describe"}
      <DescribeView
        {tabId}
        title={s.views.top.title}
        resourceKind={s.views.top.resourceKind}
        namespace={s.views.top.namespace}
        name={s.views.top.name}
        body={s.views.top.body}
        session={s} />
    {:else if s.views.top.kind === "logs"}
      {#key s.views.top}
        <LogsView
          {tabId}
          namespace={s.views.top.namespace}
          pods={s.views.top.pods}
          label={s.views.top.label}
          session={s} />
      {/key}
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
    {:else if s.views.top.kind === "rollout"}
      <RolloutView
        {tabId}
        namespace={s.views.top.namespace}
        name={s.views.top.name}
        session={s}
        onundo={requestRolloutUndo} />
    {:else if s.views.top.kind === "values"}
      {#key s.views.top}
        <ValuesView
          {tabId}
          resourceKind={s.views.top.resourceKind}
          namespace={s.views.top.namespace}
          name={s.views.top.name}
          session={s} />
      {/key}
    {/if}

    {#if actionError}
      <div class="detail-bar">
        <span class="st-bad">{actionError}</span>
        <button onclick={() => (actionError = null)}>Dismiss</button>
      </div>
    {/if}
    {#if notice}
      <div class="detail-bar">
        <span class="st-ok">{notice}</span>
        <button onclick={() => (notice = null)}>Dismiss</button>
      </div>
    {/if}

    {#if bar !== null}
      <CommandBar
        session={s}
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

    {#if menu !== null}
      <ContextMenu
        x={menu.x}
        y={menu.y}
        groups={menu.groups}
        onpick={onMenuPick}
        onclose={() => (menu = null)} />
    {/if}
  {/if}
</div>
