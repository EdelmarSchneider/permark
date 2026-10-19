<script lang="ts">
  // Files | Search: the two views of the sidebar of a project.
  import FolderTree from "@lucide/svelte/icons/folder-tree";
  import SearchIcon from "@lucide/svelte/icons/search";
  import { t } from "../i18n";
  import { ui, type SidebarView } from "../shell/ui.svelte";

  const TABS: { view: SidebarView; label: "sidebar.files" | "sidebar.search" }[] = [
    { view: "files", label: "sidebar.files" },
    { view: "search", label: "sidebar.search" },
  ];
</script>

<div class="tabs" role="tablist" aria-label={t("sidebar.views")}>
  {#each TABS as tab (tab.view)}
    <button
      type="button"
      role="tab"
      class="tab"
      aria-selected={ui.sidebar === tab.view}
      onclick={() => ui.showSidebar(tab.view)}
    >
      {#if tab.view === "files"}<FolderTree size={14} aria-hidden="true" />{:else}<SearchIcon
          size={14}
          aria-hidden="true"
        />{/if}
      {t(tab.label)}
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
