<script lang="ts">
  import type { MenuActionId, MenuItem } from "../contextMenu";

  let {
    x,
    y,
    groups,
    onpick,
    onclose,
  }: {
    x: number;
    y: number;
    groups: MenuItem[][];
    onpick: (id: MenuActionId) => void;
    onclose: () => void;
  } = $props();

  let el: HTMLDivElement | undefined = $state();
  let pos = $state({ x: 0, y: 0 });
  let ready = $state(false);

  // Clamp to the viewport once the menu has a measurable size; re-runs when a
  // new right-click moves the anchor while the menu is already open.
  $effect(() => {
    if (!el) return;
    const r = el.getBoundingClientRect();
    pos = {
      x: x + r.width > window.innerWidth - 4 ? Math.max(4, window.innerWidth - r.width - 4) : x,
      y: y + r.height > window.innerHeight - 4 ? Math.max(4, window.innerHeight - r.height - 4) : y,
    };
    ready = true;
  });

  function onWindowPointerDown(e: PointerEvent) {
    if (el && !el.contains(e.target as Node)) onclose();
  }
</script>

<svelte:window onpointerdown={onWindowPointerDown} onblur={onclose} onresize={onclose} />

<div
  class="ctxmenu"
  bind:this={el}
  style="left: {pos.x}px; top: {pos.y}px; visibility: {ready ? 'visible' : 'hidden'};"
  role="menu"
  tabindex="-1"
  oncontextmenu={(e) => e.preventDefault()}>
  {#each groups as group, gi (gi)}
    {#if gi > 0}<div class="ctxmenu-sep"></div>{/if}
    {#each group as item (item.id)}
      <button
        type="button"
        class="ctxmenu-item"
        class:danger={item.danger}
        role="menuitem"
        onclick={() => onpick(item.id)}>
        <span>{item.label}</span>
        <span class="ctxmenu-key mono">{item.key}</span>
      </button>
    {/each}
  {/each}
</div>
