<script lang="ts">
  import { matchRow } from "../command";
  import { tokenizeYamlDocument, yamlLineSegments } from "../yamlHighlight";

  let { body, query }: { body: string; query: string } = $props();

  const lines = $derived(body.split("\n"));
  // Tokenization/segmenting depends only on `body`, not `query`, so search
  // typing never re-tokenizes; only the per-line `.hl` class below reacts to it.
  const segments = $derived(
    tokenizeYamlDocument(body).map((toks, i) => yamlLineSegments(lines[i], toks)),
  );
  let container: HTMLElement | undefined = $state();

  $effect(() => {
    query;
    if (query && container) {
      container.querySelector(".hl")?.scrollIntoView({ block: "center" });
    }
  });
</script>

<pre class="hltext mono" bind:this={container}>{#each lines as l, i}<div class:hl={query ? matchRow(l, query) : false}>{#each segments[i] as seg}{#if seg.cls}<span class={"yh-" + seg.cls}>{seg.text}</span>{:else}{seg.text}{/if}{/each}{l.length === 0 ? " " : ""}</div>{/each}</pre>
