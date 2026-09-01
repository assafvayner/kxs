<script lang="ts">
  import { api, type ResourceKind } from "../api";
  import { schemaProviderFor } from "../editor/k8s/provider";
  import { settings } from "../stores/settings.svelte";
  import CodeEditor from "./CodeEditor.svelte";

  let {
    tabId,
    title,
    body,
    resourceKind,
    namespace,
    name,
    onClose,
  }: {
    tabId: number;
    title: string;
    body: string;
    resourceKind: ResourceKind;
    namespace: string | null;
    name: string;
    onClose: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally -- tabId and resourceKind are fixed for the life of the editor
  const k8s = {
    provider: schemaProviderFor(tabId),
    fallback: { group: resourceKind.group, version: resourceKind.version, kind: resourceKind.kind },
  };

  // svelte-ignore state_referenced_locally -- one-time seed of the draft from the initial prop value
  let draft = $state(body);
  // svelte-ignore state_referenced_locally -- one-time seed of the saved baseline from the initial prop value
  let saved = $state(body);
  // svelte-ignore state_referenced_locally -- one-time seed of the server baseline from the initial prop value
  // The document the edit is diffed against; only a server round-trip moves it.
  let base = $state(body);
  let dirty = $derived(draft !== saved);
  let status = $state<{ kind: "idle" | "ok" | "err"; msg: string }>({ kind: "idle", msg: "" });
  let busy = $state(false);

  const vimOn = $derived(settings.vimMode);
  let vimMode = $state("normal");
  const vimStatus = $derived(`-- ${vimMode.toUpperCase()} --`);

  let reseeding = false;
  $effect(() => {
    // any edit invalidates a prior validate/apply status, but reseeding the
    // draft from the server is not an edit
    void draft;
    if (reseeding) {
      reseeding = false;
      return;
    }
    status = { kind: "idle", msg: "" };
  });

  function reseed(yaml: string) {
    base = yaml;
    saved = yaml;
    if (draft !== yaml) {
      reseeding = true;
      draft = yaml;
    }
  }

  async function validate() {
    busy = true;
    try {
      await api.applyYaml(tabId, resourceKind, namespace, name, base, draft, true);
      status = { kind: "ok", msg: "Valid (dry-run passed)" };
    } catch (e) {
      status = { kind: "err", msg: String(e) };
    } finally {
      busy = false;
    }
  }
  async function apply(): Promise<boolean> {
    busy = true;
    try {
      const fresh = await api.applyYaml(tabId, resourceKind, namespace, name, base, draft, false);
      if (fresh === null) {
        saved = draft;
        status = { kind: "ok", msg: "No changes to apply" };
      } else {
        reseed(fresh);
        status = { kind: "ok", msg: "Applied" };
      }
      return true;
    } catch (e) {
      status = { kind: "err", msg: String(e) };
      return false;
    } finally {
      busy = false;
    }
  }
  async function reload() {
    busy = true;
    try {
      reseed(await api.getResourceYaml(tabId, resourceKind, namespace, name));
      status = { kind: "ok", msg: "Reloaded from the server" };
    } catch (e) {
      status = { kind: "err", msg: String(e) };
    } finally {
      busy = false;
    }
  }

  // :w applies, :q closes, :wq / :x apply then close (close anyway when clean)
  const exCommands = {
    write: () => {
      if (!busy && dirty) apply();
    },
    quit: () => onClose(),
    writeQuit: async () => {
      if (!busy && dirty) {
        if (await apply()) onClose();
      } else {
        onClose();
      }
    },
  };
</script>

<div class="detail">
  <div class="detail-bar">
    <span class="mono">{title}</span>
    <button onclick={validate} disabled={busy}>Validate</button>
    <button class="primary" onclick={apply} disabled={busy || !dirty}>Apply</button>
    {#if dirty}<span class="dim">● unsaved</span>{/if}
    {#if vimOn}<span class="vim-mode mono" aria-live="polite">{vimStatus}</span>{/if}
    {#if status.kind === "ok"}<span class="st-ok">{status.msg}</span>{/if}
    {#if status.kind === "err"}
      <span class="st-bad" title={status.msg}>apply failed</span>
      <button onclick={reload} disabled={busy}>Reload</button>
    {/if}
  </div>
  <CodeEditor
    bind:value={draft}
    vim={vimOn}
    autofocus
    commands={exCommands}
    onEscape={onClose}
    onVimMode={(m) => (vimMode = m)}
    {k8s} />
  {#if status.kind === "err"}<pre class="apply-err mono">{status.msg}</pre>{/if}
</div>
