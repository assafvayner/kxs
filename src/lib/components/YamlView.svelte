<script lang="ts">
  import type { TabSession } from "../stores/sessions.svelte";
  import CodeEditor from "./CodeEditor.svelte";

  let { title, body, session }: { title: string; body: string; session: TabSession } = $props();
</script>

<div class="detail">
  <div class="detail-bar"><span class="mono">{title}</span></div>
  <CodeEditor value={body} readOnly filter={session.filter} />
</div>
