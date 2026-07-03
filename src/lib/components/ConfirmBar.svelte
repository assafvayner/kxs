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
  $effect(() => { (kind === "number" ? input : cancelBtn)?.focus(); });
  function onkeydown(e: KeyboardEvent) {
    e.stopPropagation(); // this bar owns the keyboard while open
    if (e.key === "Escape") { e.preventDefault(); oncancel(); }
    else if (e.key === "Enter" && kind === "number") { e.preventDefault(); onconfirm(num); }
    // confirm kind: Enter activates the focused button natively (Cancel by default)
  }
</script>
<div class="confirmbar">
  <span>{message}</span>
  {#if kind === "number"}
    <input bind:this={input} {onkeydown} type="number" min="0" bind:value={num} class="mono" />
  {/if}
  <button class="primary" {onkeydown} onclick={() => onconfirm(kind === "number" ? num : undefined)}>Confirm</button>
  <button bind:this={cancelBtn} {onkeydown} onclick={oncancel}>Cancel</button>
</div>
