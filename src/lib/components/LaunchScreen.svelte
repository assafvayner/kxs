<script lang="ts">
  import { contexts } from "../stores/contexts.svelte";
  import { tabs } from "../stores/tabs.svelte";
  import { api } from "../api";
  import { matchRow } from "../command";
  import ContextForm from "./ContextForm.svelte";

  // undefined = list view; null = create form; string = edit form for that name
  let editing = $state<string | null | undefined>(undefined);
  let confirmingDelete = $state<string | null>(null);

  let filter = $state("");
  const visible = $derived(
    contexts.view.contexts.filter((ctx) =>
      [ctx.name, ctx.cluster, ctx.user, ctx.namespace ?? ""].some((v) => matchRow(v, filter)),
    ),
  );

  async function remove(name: string) {
    try {
      await api.deleteContext(name);
      await contexts.refresh();
    } catch (e) {
      contexts.error = String(e);
    } finally {
      confirmingDelete = null;
    }
  }

  // Pings are on-demand only: exec-auth plugins can trigger SSO prompts, so
  // we never auto-ping every context at launch.
  type Ping = { state: "idle" | "checking" | "ok" | "fail"; detail: string };
  let pings = $state<Record<string, Ping>>({});

  async function pingRow(name: string) {
    pings[name] = { state: "checking", detail: "" };
    try {
      const version = await api.pingContext(name);
      pings[name] = { state: "ok", detail: version };
    } catch (e) {
      pings[name] = { state: "fail", detail: String(e) };
    }
  }
</script>

{#if editing !== undefined}
  <ContextForm
    name={editing}
    onclose={() => {
      editing = undefined;
      contexts.refresh();
    }} />
{:else}
  <div class="launch">
    <header>
      <h1>kxs</h1>
      <button class="primary" onclick={() => (editing = null)}>New context</button>
    </header>

    {#if contexts.error}
      <div class="error">{contexts.error}</div>
    {/if}
    {#each contexts.view.warnings as w}
      <div class="warning">{w}</div>
    {/each}

    {#if contexts.view.contexts.length === 0}
      <p class="dim">
        No contexts found in {contexts.view.files.join(", ") || "any kubeconfig"}.
        Create one to get started.
      </p>
    {:else}
      <div class="searchbar launch-filter">
        <span class="mag">🔍</span>
        <input
          bind:value={filter}
          placeholder="filter contexts (-r for regex)"
          class="mono"
          onkeydown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              if (filter) filter = "";
              else (e.currentTarget as HTMLInputElement).blur();
            }
          }} />
        {#if filter}
          <button type="button" class="clear" onclick={() => (filter = "")} title="clear">×</button>
        {/if}
      </div>
      {#if visible.length === 0}
        <p class="dim">No contexts match “{filter}”.</p>
      {/if}
      <table>
        <thead>
          <tr><th></th><th></th><th>Context</th><th>Cluster</th><th>User</th><th>Namespace</th><th>Source</th><th></th></tr>
        </thead>
        <tbody>
          {#each visible as ctx (ctx.name)}
            <tr
              class="clickable"
              onclick={() => tabs.open(ctx.name)}
              tabindex="0"
              onkeydown={(e) => {
                if (e.target !== e.currentTarget) return;
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  tabs.open(ctx.name);
                }
              }}>
              <td>{ctx.name === contexts.view.currentContext ? "★" : ""}</td>
              <td class="ping">
                {#if pings[ctx.name]?.state === "ok"}
                  <span class="dot ok" title={pings[ctx.name].detail}></span>
                {:else if pings[ctx.name]?.state === "fail"}
                  <span class="dot fail" title={pings[ctx.name].detail}></span>
                {:else if pings[ctx.name]?.state === "checking"}
                  <span class="dot busy" title="checking…"></span>
                {:else}
                  <button
                    class="ping-btn"
                    title="Check reachability"
                    onclick={(e) => {
                      e.stopPropagation();
                      pingRow(ctx.name);
                    }}>◌</button>
                {/if}
              </td>
              <td><strong>{ctx.name}</strong></td>
              <td>{ctx.cluster}</td>
              <td>{ctx.user}</td>
              <td>{ctx.namespace ?? "—"}</td>
              <td class="mono dim">{ctx.source}</td>
              <td class="actions">
                {#if confirmingDelete === ctx.name}
                  <button
                    class="danger"
                    title="Removes only the context entry; cluster and user entries are kept"
                    onclick={(e) => {
                      e.stopPropagation();
                      remove(ctx.name);
                    }}>Confirm delete</button>
                  <button
                    onclick={(e) => {
                      e.stopPropagation();
                      confirmingDelete = null;
                    }}>Keep</button>
                {:else}
                  <button
                    onclick={(e) => {
                      e.stopPropagation();
                      editing = ctx.name;
                    }}>Edit</button>
                  <button
                    class="danger"
                    onclick={(e) => {
                      e.stopPropagation();
                      confirmingDelete = ctx.name;
                    }}>Delete</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
{/if}
