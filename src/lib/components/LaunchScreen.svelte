<script lang="ts">
  import { contexts } from "../stores/contexts.svelte";
  import { tabs } from "../stores/tabs.svelte";
  import { api } from "../api";
  import ContextForm from "./ContextForm.svelte";

  // undefined = list view; null = create form; string = edit form for that name
  let editing = $state<string | null | undefined>(undefined);

  async function remove(name: string) {
    if (!confirm(`Delete context "${name}"? The cluster and user entries are kept.`)) return;
    try {
      await api.deleteContext(name);
      await contexts.refresh();
    } catch (e) {
      contexts.error = String(e);
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
      <div class="error">{w}</div>
    {/each}

    {#if contexts.view.contexts.length === 0}
      <p class="dim">
        No contexts found in {contexts.view.files.join(", ") || "any kubeconfig"}.
        Create one to get started.
      </p>
    {:else}
      <table>
        <thead>
          <tr><th></th><th>Context</th><th>Cluster</th><th>User</th><th>Namespace</th><th>Source</th><th></th></tr>
        </thead>
        <tbody>
          {#each contexts.view.contexts as ctx (ctx.name)}
            <tr class="clickable" onclick={() => tabs.open(ctx.name)}>
              <td>{ctx.name === contexts.view.currentContext ? "★" : ""}</td>
              <td><strong>{ctx.name}</strong></td>
              <td>{ctx.cluster}</td>
              <td>{ctx.user}</td>
              <td>{ctx.namespace ?? "—"}</td>
              <td class="mono dim">{ctx.source}</td>
              <td class="actions">
                <button
                  onclick={(e) => {
                    e.stopPropagation();
                    editing = ctx.name;
                  }}>Edit</button>
                <button
                  class="danger"
                  onclick={(e) => {
                    e.stopPropagation();
                    remove(ctx.name);
                  }}>Delete</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
{/if}
