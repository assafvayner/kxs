<script lang="ts">
  let { context }: { context: string } = $props();
</script>

<div class="cluster-tab">
  <h2>{context}</h2>
  <p class="dim">Cluster views land in Plan 2. This tab holds the session for <span class="mono">{context}</span>.</p>
</div>
