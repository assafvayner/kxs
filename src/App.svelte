<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import TabBar from "./lib/components/TabBar.svelte";
  import LaunchScreen from "./lib/components/LaunchScreen.svelte";
  import ClusterTab from "./lib/components/ClusterTab.svelte";
  import { tabs } from "./lib/stores/tabs.svelte";
  import { contexts } from "./lib/stores/contexts.svelte";
  import { handleKeydown, isEditableTarget } from "./lib/keys";

  onMount(() => {
    contexts.refresh();
    const unlisten = listen("kubeconfig://changed", () => contexts.refresh());
    return () => {
      unlisten.then((f) => f());
    };
  });

  const active = $derived(tabs.tabs.find((t) => t.id === tabs.activeId) ?? null);

  function onKeydown(e: KeyboardEvent) {
    // shortcuts must not fire while typing; ctrl+tab cycling is always safe
    if (isEditableTarget(e.target) && !(e.ctrlKey && e.key === "Tab")) return;
    handleKeydown(e, tabs);
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="app">
  <TabBar />
  <main>
    {#if active}
      {#key active.id}
        <ClusterTab context={active.context} />
      {/key}
    {:else}
      <LaunchScreen />
    {/if}
  </main>
</div>
