<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import TabBar from "./lib/components/TabBar.svelte";
  import LaunchScreen from "./lib/components/LaunchScreen.svelte";
  import ClusterTab from "./lib/components/ClusterTab.svelte";
  import SettingsView from "./lib/components/SettingsView.svelte";
  import { tabs } from "./lib/stores/tabs.svelte";
  import { contexts } from "./lib/stores/contexts.svelte";
  import { handleKeydown, isEditableTarget } from "./lib/keys";
  import { clusterKeyHandlers } from "./lib/clusterKeys";

  onMount(() => {
    contexts.refresh();
    const unlisten = listen("kubeconfig://changed", () => contexts.refresh());
    return () => {
      unlisten.then((f) => f());
    };
  });

  function onKeydown(e: KeyboardEvent) {
    // shortcuts must not fire while typing; ctrl+tab cycling is always safe
    if (isEditableTarget(e.target) && !(e.ctrlKey && e.key === "Tab")) return;
    // active cluster tab gets first crack at cluster keys via a per-tab handler
    if (typeof tabs.activeId === "number" && clusterKeyHandlers.get(tabs.activeId)?.(e)) return;
    handleKeydown(e, tabs);
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="app">
  <TabBar />
  <main>
    {#each tabs.tabs as tab (tab.id)}
      <div class="pane" hidden={tabs.activeId !== tab.id}>
        <ClusterTab context={tab.context} tabId={tab.id} />
      </div>
    {/each}
    <div class="pane" hidden={tabs.activeId !== null}>
      <LaunchScreen />
    </div>
    <div class="pane" hidden={tabs.activeId !== "settings"}>
      <SettingsView />
    </div>
  </main>
</div>
