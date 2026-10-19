<script lang="ts">
  // Files | Search: the two views of the sidebar of a project.
  import FolderTree from "@lucide/svelte/icons/folder-tree";
  import SearchIcon from "@lucide/svelte/icons/search";
  import { t } from "../i18n";
  import { activeSidebar, leftPanels } from "../shell/panels.svelte";
  import { ui } from "../shell/ui.svelte";

  const plugins = $derived(leftPanels());
  const shown = $derived(activeSidebar());
</script>

<div class="tabs" role="tablist" aria-label={t("sidebar.views")}>
  <button
    type="button"
    role="tab"
    class="tab"
    aria-selected={shown === "files"}
    onclick={() => ui.showSidebar("files")}
  >
    <FolderTree size={14} aria-hidden="true" />
    {t("sidebar.files")}
  </button>
  <button
    type="button"
    role="tab"
    class="tab"
    aria-selected={shown === "search"}
    onclick={() => ui.showSidebar("search")}
  >
    <SearchIcon size={14} aria-hidden="true" />
    {t("sidebar.search")}
  </button>
  {#each plugins as panel (panel.owner + panel.id)}
    <button
      type="button"
      role="tab"
      class="tab"
      aria-selected={shown === panel.id}
      onclick={() => ui.showSidebar(panel.id)}
    >
      {t(panel.label)}
    </button>
  {/each}
</div>

<style>
  .tabs {
    display: flex;
    flex: none;
    gap: 2px;
    padding: var(--sp-2) var(--sp-2) 0;
    border-bottom: 1px solid var(--border);
  }
  .tab {
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
  .tab:hover {
    color: var(--text);
  }
  .tab[aria-selected="true"] {
    border-bottom-color: var(--accent);
    color: var(--text);
    font-weight: 600;
  }
</style>
