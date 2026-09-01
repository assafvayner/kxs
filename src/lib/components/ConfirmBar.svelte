<script lang="ts">
  import type { DeleteOptions, PropagationPolicy } from "../api";
  let { message, kind, onconfirm, oncancel }: {
    message: string;
    kind: "confirm" | "number" | "delete";
    onconfirm: (value?: number, opts?: DeleteOptions) => void;
    oncancel: () => void;
  } = $props();
  let num = $state(1);
  let propagation = $state<PropagationPolicy>("Background");
  let force = $state(false);
  let input: HTMLInputElement | undefined = $state();
  let cancelBtn: HTMLButtonElement | undefined = $state();
  $effect(() => { (kind === "number" ? input : cancelBtn)?.focus(); });
  function accept() {
    onconfirm(
      kind === "number" ? num : undefined,
      kind === "delete" ? { propagation, force } : undefined,
    );
  }
  function onkeydown(e: KeyboardEvent) {
    e.stopPropagation(); // this bar owns the keyboard while open
    if (e.key === "Escape") { e.preventDefault(); oncancel(); }
    // buttons: Enter activates the focused button natively (Cancel by default)
  }
  function onfieldkeydown(e: KeyboardEvent) {
    onkeydown(e);
    if (e.key === "Enter") { e.preventDefault(); accept(); }
  }
</script>
<div class="confirmbar">
  <span>{message}</span>
  {#if kind === "number"}
    <input bind:this={input} onkeydown={onfieldkeydown} type="number" min="0" bind:value={num} class="mono" />
  {/if}
  {#if kind === "delete"}
    <label class="del-opt">
      cascade
      <select onkeydown={onfieldkeydown} bind:value={propagation}>
        <option value="Background">Background</option>
        <option value="Foreground">Foreground</option>
        <option value="Orphan">Orphan</option>
      </select>
    </label>
    <label class="del-opt">
      <input onkeydown={onfieldkeydown} type="checkbox" bind:checked={force} />
      force
    </label>
  {/if}
  <button class="primary" {onkeydown} onclick={accept}>Confirm</button>
  <button bind:this={cancelBtn} {onkeydown} onclick={oncancel}>Cancel</button>
</div>
