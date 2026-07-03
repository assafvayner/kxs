<script lang="ts">
  let { message, kind, onconfirm, oncancel }: {
    message: string;
    kind: "confirm" | "number";
    onconfirm: (value?: number) => void;
    oncancel: () => void;
  } = $props();
  let num = $state(1);
  let input: HTMLInputElement | undefined = $state();
  let cancelBtn: HTMLButtonElement | undefined = $state();
  $effect(() => {
    if (kind === "number") input?.focus();
    else cancelBtn?.focus();
  });
  function onkeydown(e: KeyboardEvent) {
    // stopPropagation: the confirm/cancel buttons aren't "editable" targets, so
    // without this the window-level cluster key handler would also see Enter/Escape.
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); oncancel(); }
    else if (e.key === "Enter") { e.preventDefault(); e.stopPropagation(); onconfirm(kind === "number" ? num : undefined); }
  }
</script>

<div class="confirmbar">
  <span>{message}</span>
  {#if kind === "number"}
    <input bind:this={input} type="number" min="0" bind:value={num} class="mono" {onkeydown} />
  {/if}
  <button
    class="primary"
    {onkeydown}
    onclick={() => onconfirm(kind === "number" ? num : undefined)}>Confirm</button>
  <button bind:this={cancelBtn} {onkeydown} onclick={oncancel}>Cancel</button>
</div>
