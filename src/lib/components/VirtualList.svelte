<script lang="ts" generics="T">
  import type { Snippet } from "svelte";
  import { windowRange } from "../virtual";

  let {
    items,
    itemHeight = 28,
    row,
  }: { items: T[]; itemHeight?: number; row: Snippet<[T]> } = $props();

  let viewport: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewportHeight = $state(400);

  const range = $derived(windowRange(scrollTop, viewportHeight, itemHeight, items.length));
  const visible = $derived(items.slice(range.start, range.end));

  function onscroll() {
    if (viewport) scrollTop = viewport.scrollTop;
  }
</script>

<div
  class="vlist"
  bind:this={viewport}
  bind:clientHeight={viewportHeight}
  {onscroll}>
  <div style="height: {range.padTop}px"></div>
  {#each visible as item ((item as { key?: unknown }).key ?? item)}
    {@render row(item)}
  {/each}
  <div style="height: {range.padBottom}px"></div>
</div>
