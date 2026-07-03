<script lang="ts">
  let { context, tabId }: { context: string; tabId: number } = $props();
</script>

<div class="cluster-tab">
  <h2>{context}</h2>
  <p class="dim">Connecting UI lands in Task 9 (tab {tabId}).</p>
</div>
