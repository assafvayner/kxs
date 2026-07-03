<script lang="ts">
  let { name = null, onclose }: { name?: string | null; onclose: () => void } = $props();
</script>
<div class="launch"><p>Form for {name ?? "new context"}</p><button onclick={onclose}>Back</button></div>
