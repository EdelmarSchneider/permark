<script lang="ts">
  // Files | Search: the two views of the sidebar of a project.
  import FolderTree from "@lucide/svelte/icons/folder-tree";
  import SearchIcon from "@lucide/svelte/icons/search";
  import { t } from "../i18n";
  import { activeSidebar, leftPanels } from "../shell/panels.svelte";
  import { ui } from "../shell/ui.svelte";

  const plugins = $derived(leftPanels());
  const shown = $derived(activeSidebar());
  /** Many tabs: the ones that are not open show their icon alone (the label is the tooltip). */
  const crowded = $derived(plugins.length + 2 > 3);
</script>

<div class="tabs" class:crowded role="tablist" aria-label={t("sidebar.views")}>
  <button
    type="button"
    role="tab"
    class="tab"
    class:icon-only={crowded && shown !== "files"}
    aria-selected={shown === "files"}
    aria-label={t("sidebar.files")}
    title={t("sidebar.files")}
    onclick={() => ui.showSidebar("files")}
  >
    <FolderTree size={14} aria-hidden="true" />
    {#if !crowded || shown === "files"}<span class="label">{t("sidebar.files")}</span>{/if}
  </button>
  <button
    type="button"
    role="tab"
    class="tab"
    class:icon-only={crowded && shown !== "search"}
    aria-selected={shown === "search"}
    aria-label={t("sidebar.search")}
    title={t("sidebar.search")}
    onclick={() => ui.showSidebar("search")}
  >
    <SearchIcon size={14} aria-hidden="true" />
    {#if !crowded || shown === "search"}<span class="label">{t("sidebar.search")}</span>{/if}
  </button>
  {#each plugins as panel (panel.owner + panel.id)}
    <button
      type="button"
      role="tab"
      class="tab"
      class:icon-only={panel.icon !== undefined && crowded && shown !== panel.id}
      aria-selected={shown === panel.id}
      aria-label={t(panel.label)}
      title={t(panel.label)}
      onclick={() => ui.showSidebar(panel.id)}
    >
      {#if panel.icon}
        {@const Icon = panel.icon}
        <Icon size={14} />
      {/if}
      {#if !panel.icon || !crowded || shown === panel.id}<span class="label">{t(panel.label)}</span
        >{/if}
    </button>
  {/each}
</div>

<style>
  .tabs {
    display: flex;
    flex: none;
    gap: 2px;
    padding: var(--sp-2) var(--sp-2) 0;
    overflow: hidden;
    border-bottom: 1px solid var(--border);
  }
  /* the open tab gives way before the others do: every tab stays reachable */
  .tab {
    flex: 0 1 auto;
    min-width: 0;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 5px var(--sp-3);
    border: 0;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .tab.icon-only {
    flex: none;
    padding-inline: var(--sp-2);
  }
  /* many tabs: tighter, so the open tab's label has room for long words (German, Italian); icons stay 24 px wide */
  .crowded .tab {
    padding-inline: var(--sp-2);
  }
  .crowded .tab.icon-only {
    padding-inline: 5px;
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tab:hover {
    color: var(--text);
  }
  .tab[aria-selected="true"] {
    border-bottom-color: var(--accent);
    color: var(--text);
    font-weight: 600;
  }
</style>
