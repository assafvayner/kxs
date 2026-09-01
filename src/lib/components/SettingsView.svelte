<script lang="ts">
  import { settings } from "../stores/settings.svelte";
  import { tabs } from "../stores/tabs.svelte";
  import { THEMES, type Theme } from "../themes";

  const all = Object.values(THEMES);
  const groups: { title: string; themes: Theme[] }[] = [
    { title: "Dark", themes: all.filter((t) => t.dark) },
    { title: "Light", themes: all.filter((t) => !t.dark) },
  ];

  function swatches(t: Theme): string[] {
    const c = t.colors;
    return [c.bg, c.bgActive, c.accent, c.green, c.red];
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === "Escape" && settings.previewTheme !== null) {
      settings.setPreviewTheme(null);
      e.stopPropagation();
    }
  }

  // the pane is hidden, not unmounted, when another tab is active — a hover
  // preview would otherwise stay applied with no mouseleave to clear it
  $effect(() => {
    if (tabs.activeId !== "settings") settings.setPreviewTheme(null);
  });
</script>

<svelte:window onblur={() => settings.setPreviewTheme(null)} />

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

    <h3 class="settings-h">Theme</h3>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="theme-list"
      onmouseleave={() => settings.setPreviewTheme(null)}
      onkeydown={onKeydown}>
      {#each groups as group (group.title)}
        <div class="theme-group">{group.title}</div>
        {#each group.themes as t (t.id)}
          <button
            class="theme-row"
            class:current={settings.theme === t.id}
            aria-pressed={settings.theme === t.id}
            onmouseenter={() => settings.setPreviewTheme(t.id)}
            onfocus={() => settings.setPreviewTheme(t.id)}
            onblur={() => settings.setPreviewTheme(null)}
            onclick={() => settings.setTheme(t.id)}>
            <span class="theme-name">{t.label}</span>
            <span class="theme-swatches">
              {#each swatches(t) as color, i (i)}
                <span class="theme-swatch" style="background: {color}"></span>
              {/each}
            </span>
            <span class="theme-check" class:visible={settings.theme === t.id}>✓</span>
          </button>
        {/each}
      {/each}
    </div>
  </div>
</div>
