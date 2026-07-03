<script lang="ts">
  import { tabs } from "../stores/tabs.svelte";
</script>

<nav class="tabbar">
  <button
    class="tab home"
    class:active={tabs.activeId === null}
    onclick={() => tabs.activate(null)}
    title="Contexts (⌘T)">⌂</button>
  {#each tabs.tabs as tab (tab.id)}
    <div
      class="tab"
      class:active={tabs.activeId === tab.id}
      role="button"
      tabindex="0"
      onclick={() => tabs.activate(tab.id)}
      onkeydown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          tabs.activate(tab.id);
        }
      }}>
      <span class="dot"></span>
      <span class="name">{tab.context}</span>
      <button
        class="close"
        aria-label="Close"
        title="Close (⌘W)"
        onclick={(e) => {
          e.stopPropagation();
          tabs.close(tab.id);
        }}>×</button>
    </div>
  {/each}
</nav>
