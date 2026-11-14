<script lang="ts">
  // Files | Search | the plugins' panels: the views of the sidebar of a project, as the right column's tab strip
  // (one Tab stop, arrows between the tabs).
  import FolderTree from "@lucide/svelte/icons/folder-tree";
  import SearchIcon from "@lucide/svelte/icons/search";
  import { t } from "../i18n";
  import { activeSidebar, leftPanels } from "../shell/panels.svelte";
  import ViewTabs, { type TabView } from "../shell/ViewTabs.svelte";
  import { ui } from "../shell/ui.svelte";

  const views = $derived<TabView[]>([
    { id: "files", label: t("sidebar.files"), icon: FolderTree, badge: null },
    { id: "search", label: t("sidebar.search"), icon: SearchIcon, badge: null },
    ...leftPanels().map((panel) => ({
      id: panel.id,
      label: t(panel.label),
      icon: panel.icon,
      badge: null,
    })),
  ]);
</script>

<!-- three tabs keep their labels; more show the closed ones as icons -->
<ViewTabs
  {views}
  selected={activeSidebar()}
  onselect={(id) => ui.showSidebar(id)}
  label={t("sidebar.views")}
  roomy={3}
/>
