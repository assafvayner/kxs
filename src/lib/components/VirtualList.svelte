<script lang="ts" generics="T">
  import type { Snippet } from "svelte";
  import { windowRange } from "../virtual";

  let {
    items,
    itemHeight = 28,
    row,
    scrollToIndex = null,
  }: { items: T[]; itemHeight?: number; row: Snippet<[T]>; scrollToIndex?: number | null } =
    $props();

  let viewport: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewportHeight = $state(400);

  const range = $derived(windowRange(scrollTop, viewportHeight, itemHeight, items.length));
  const visible = $derived(items.slice(range.start, range.end));

  function onscroll() {
    if (viewport) scrollTop = viewport.scrollTop;
  }

  // Keep the given row inside the viewport (keyboard selection follows j/k).
  $effect(() => {
    if (scrollToIndex === null || scrollToIndex < 0 || !viewport) return;
    const top = scrollToIndex * itemHeight;
    const bottom = top + itemHeight;
    if (top < viewport.scrollTop) {
      viewport.scrollTop = top;
      scrollTop = top;
    } else if (bottom > viewport.scrollTop + viewport.clientHeight) {
      const t = bottom - viewport.clientHeight;
      viewport.scrollTop = t;
      scrollTop = t;
    }
  });
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
