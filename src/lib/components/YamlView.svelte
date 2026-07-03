<script lang="ts">
  let { title, body }: { title: string; body: string } = $props();
  let query = $state("");
  const lines = $derived(body.split("\n"));
  const matches = $derived(
    query ? lines.reduce((n, l) => n + (l.toLowerCase().includes(query.toLowerCase()) ? 1 : 0), 0) : 0,
  );
</script>

<div class="detail">
  <div class="detail-bar">
    <span class="mono">{title}</span>
    <input class="mono" placeholder="/ search" bind:value={query} />
    {#if query}<span class="dim">{matches} matching lines</span>{/if}
  </div>
  <pre class="detail-body mono">{#each lines as l}<div class:hl={query && l.toLowerCase().includes(query.toLowerCase())}>{l || " "}</div>{/each}</pre>
</div>
