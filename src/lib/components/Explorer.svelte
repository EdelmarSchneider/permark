<script lang="ts">
  import { untrack } from "svelte";
  import { LOCALE, t, tn } from "../i18n";
  import { app } from "../stores/app.svelte";
  import { docs } from "../stores/docs.svelte";
  import { filter } from "../stores/filter.svelte";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import FilterPopover from "./FilterPopover.svelte";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import Tree from "./Tree.svelte";

  let tree: Tree | undefined = $state();
  let menuAnchor = $state<DOMRect | null>(null);
  let filterAnchor = $state<DOMRect | null>(null);

  const projectOpen = $derived(vault.stateLocation === "project");
  const entries = $derived<MenuEntry[]>([
    { id: "collapse", label: t("explorer.collapseAll"), onselect: () => vault.collapseAll() },
    {
      id: "reveal",
      label: t("explorer.revealOpen"),
      disabled: !docs.current,
      onselect: () => docs.path && vault.revealPath(docs.path),
    },
    {
      id: "autoReveal",
      label: t("explorer.autoReveal"),
      checked: session.autoReveal,
      onselect: () => session.toggleAutoReveal(),
    },
    { type: "separator" },
    {
      id: "init",
      label: projectOpen ? t("explorer.stateInProject") : t("explorer.initialize"),
      icon: "folder-cog",
      disabled: projectOpen,
      onselect: () => (ui.initDialog = true),
    },
  ]);

  const filterNames = {
    documents: "filter.documents",
    "docs-and-assets": "filter.docsAndAssets",
    everything: "filter.everything",
    custom: "filter.custom",
  } as const;
  const filterName = $derived(t(filterNames[filter.current.show]));
  const footer = $derived(
    filter.showAll
      ? tn("filter.showingAll", filter.total)
      : tn("filter.showingOf", filter.total, {
          shown: filter.shownByFilter.toLocaleString(LOCALE),
        }),
  );

  $effect(() => {
    void ui.explorerFocusTick;
    untrack(() => {
      if (ui.explorerFocusTick > 0) tree?.focus();
    });
  });

  function toggleFilter(e: MouseEvent): void {
    filterAnchor = filterAnchor ? null : (e.currentTarget as HTMLElement).getBoundingClientRect();
  }

  function toggleMenu(e: MouseEvent): void {
    menuAnchor = menuAnchor ? null : (e.currentTarget as HTMLElement).getBoundingClientRect();
  }
</script>

<div class="panel-header">
  <span class="panel-title">{t("explorer.title")}</span>
  <button
    type="button"
    class="filter-btn"
    aria-label={t("filter.buttonLabel", { name: filterName })}
    aria-haspopup="dialog"
    aria-expanded={filterAnchor !== null}
    title={t("filter.hint")}
    data-popover-trigger
    onclick={toggleFilter}
  >
    <Icon name="filter" size="xs" />
    <span class="filter-btn__label">{filterName}</span>
  </button>
  <IconButton
    small
    icon="more"
    label={t("explorer.menu")}
    hint={t("explorer.menuHint")}
    data-popover-trigger
    onclick={toggleMenu}
  />
  <IconButton
    small
    icon="chevrons-left"
    label={t("panel.collapseLeft")}
    hint={t("panel.collapseLeftHint")}
    onclick={() => app.toggleLeft()}
  />
</div>
<div class="scroll" style="flex:1">
  <button
    class="section-header"
    aria-expanded={!ui.collapsedSections.has("vault")}
    onclick={() => ui.toggleSection("vault")}
  >
    <Icon name="chevron-down" size="sm" class="twisty" />{vault.info?.name}
  </button>
  {#if !ui.collapsedSections.has("vault")}
    {#if filter.roots.main.length > 0 || ui.newFile?.folder === ""}
      <Tree bind:this={tree} nodes={filter.roots.main} label={t("tree.files")} root />
    {:else}
      <p class="panel-note">
        {filter.hiddenFiles.length > 0 ? t("filter.noneVisible") : t("explorer.empty")}
      </p>
    {/if}
  {/if}

  {#if filter.roots.worktrees.length > 0}
    <button
      class="section-header"
      aria-expanded={!ui.collapsedSections.has("worktrees")}
      onclick={() => ui.toggleSection("worktrees")}
    >
      <Icon name="chevron-down" size="sm" class="twisty" />{t("explorer.worktrees")}
      <span
        class="section-header__aside"
        title={tn("explorer.worktreeCount", filter.roots.worktrees.length)}
      >
        {filter.roots.worktrees.length}
      </span>
    </button>
    {#if !ui.collapsedSections.has("worktrees")}
      <Tree nodes={filter.roots.worktrees} label={t("tree.worktrees")} badges={false} />
    {/if}
  {/if}
</div>
{#if filter.hiddenByFilter > 0}
  <div class="tree-footer">
    <span class="tree-footer__text">{footer}</span>
    <button type="button" class="link-btn" onclick={() => filter.setShowAll(!filter.showAll)}>
      {filter.showAll ? t("filter.useFilter") : t("filter.showAll")}
    </button>
  </div>
{/if}

{#if filterAnchor}
  <FilterPopover anchor={filterAnchor} onclose={() => (filterAnchor = null)} />
{/if}

{#if menuAnchor}
  <Menu
    label={t("explorer.menu")}
    anchor={menuAnchor}
    align="end"
    {entries}
    onclose={() => (menuAnchor = null)}
  />
{/if}
