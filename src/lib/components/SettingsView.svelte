<script lang="ts">
  import { settings } from "../stores/settings.svelte";
</script>

<div class="detail">
  <div class="detail-bar">
    <span class="mono">Settings</span>
  </div>
  <div class="settings-body">
    <label class="setting-row">
      <input
        type="checkbox"
        checked={settings.vimMode}
        onchange={(e) => settings.setVimMode(e.currentTarget.checked)} />
      <span>Enable vim keybindings in editor</span>
    </label>
  </div>
</div>
