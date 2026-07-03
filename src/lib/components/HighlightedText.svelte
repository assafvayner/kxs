<script lang="ts">
  import { matchRow } from "../command";

  let { body, query }: { body: string; query: string } = $props();

  const lines = $derived(body.split("\n"));
  let container: HTMLElement | undefined = $state();

  $effect(() => {
    query;
    if (query && container) {
      container.querySelector(".hl")?.scrollIntoView({ block: "center" });
    }
  });
</script>

<pre class="hltext mono" bind:this={container}>{#each lines as l}<div class:hl={query ? matchRow(l, query) : false}>{l || " "}</div>{/each}</pre>
