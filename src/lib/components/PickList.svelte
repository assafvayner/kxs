<script lang="ts">
  import type { PickOption } from "../containers";

  let {
    title,
    options,
    onpick,
    onclose,
  }: {
    title: string;
    options: PickOption[];
    onpick: (index: number) => void;
    onclose: () => void;
  } = $props();

  let el: HTMLDivElement | undefined = $state();
  let active = $state(0);

  $effect(() => {
    el?.focus();
  });

  function onkeydown(e: KeyboardEvent) {
    e.stopPropagation(); // this list owns the keyboard while open
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "ArrowDown" || e.key === "j") {
      e.preventDefault();
      active = Math.min(options.length - 1, active + 1);
    } else if (e.key === "ArrowUp" || e.key === "k") {
      e.preventDefault();
      active = Math.max(0, active - 1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      onpick(active);
    }
  }

  function onWindowPointerDown(e: PointerEvent) {
    if (el && !el.contains(e.target as Node)) onclose();
  }
</script>

<svelte:window onpointerdown={onWindowPointerDown} />

<div class="picklist" bind:this={el} role="menu" tabindex="-1" {onkeydown}>
  <div class="picklist-title dim">{title}</div>
  {#each options as opt, i (i)}
    <button
      type="button"
      class="ctxmenu-item"
      class:active={i === active}
      role="menuitem"
      onmouseenter={() => (active = i)}
      onclick={() => onpick(i)}>
      <span>{opt.label}</span>
      {#if opt.hint}<span class="ctxmenu-key mono">{opt.hint}</span>{/if}
    </button>
  {/each}
</div>
