<script lang="ts">
  import { untrack } from "svelte";
  import { targetFolder } from "../editor/newFile";
  import { formatNumber, t, tn } from "../i18n";
  import { app } from "../stores/app.svelte";
  import { basename } from "../paths";
  import { docs } from "../stores/docs.svelte";
  import { fileops } from "../stores/fileops.svelte";
  import { filter } from "../stores/filter.svelte";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import { unread } from "../stores/unread.svelte";
  import { vault } from "../stores/vault.svelte";
  import FilterPopover from "./FilterPopover.svelte";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import Tree from "./Tree.svelte";

  let tree: Tree | undefined = $state();
  let menuAnchor = $state<DOMRect | null>(null);
  let rootMenu = $state<{ x: number; y: number } | null>(null);
  /** Where the menu was opened from: what an entry opens (the git legend) is placed there, after the menu has closed. */
  let menuOrigin: DOMRect | null = null;
  let filterAnchor = $state<DOMRect | null>(null);

  const projectOpen = $derived(vault.stateLocation === "project");
  const entries = $derived<MenuEntry[]>([
    {
      id: "newMarkdown",
      label: t("cmd.newFile"),
      icon: "file-plus",
      onselect: () => app.startNewFile(targetFolder(ui.treeCursor, docs.path), "markdown"),
    },
    {
      id: "newText",
      label: t("cmd.newTextFile"),
      icon: "file-plus",
      onselect: () => app.startNewFile(targetFolder(ui.treeCursor, docs.path), "text"),
    },
    { type: "separator" },
    {
      id: "newFolder",
      label: t("fileOps.newFolder"),
      icon: "folder-plus",
      onselect: () => fileops.startNewFolder(targetFolder(ui.treeCursor, docs.path)),
    },
    { type: "separator" },
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
      id: "markAllRead",
      label: t("unread.markAllRead"),
      disabled: unread.count === 0,
      onselect: () => void unread.markAllRead(),
    },
    {
      id: "onlyUnread",
      label: t("unread.showOnlyUnread"),
      checked: unread.onlyUnread,
      onselect: () => unread.toggleOnlyUnread(),
    },
    { type: "separator" },
    ...(vault.git?.isRepo
      ? [
          {
            id: "gitLegend",
            label: t("git.legend.menu"),
            icon: "info",
            onselect: () => {
              if (menuOrigin) ui.gitLegend = { anchor: menuOrigin, from: "explorer" };
            },
          } satisfies MenuEntry,
        ]
      : []),
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
          shown: formatNumber(filter.shownByFilter),
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

  const rootEntries = $derived<MenuEntry[]>([
    {
      id: "newMarkdown",
      label: t("cmd.newFile"),
      icon: "file-plus",
      onselect: () => app.startNewFile("", "markdown"),
    },
    {
      id: "newText",
      label: t("cmd.newTextFile"),
      icon: "file-plus",
      onselect: () => app.startNewFile("", "text"),
    },
    { type: "separator" },
    {
      id: "newFolder",
      label: t("fileOps.newFolder"),
      icon: "folder-plus",
      onselect: () => fileops.startNewFolder(""),
    },
  ]);

  function onRootMenu(e: MouseEvent): void {
    e.preventDefault();
    rootMenu = { x: e.clientX, y: e.clientY };
  }

  function toggleMenu(e: MouseEvent): void {
    menuAnchor = menuAnchor ? null : (e.currentTarget as HTMLElement).getBoundingClientRect();
    if (menuAnchor) menuOrigin = menuAnchor;
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
    title={`${filterName} · ${t("filter.hint")}`}
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
<div
  class={["scroll", "scroll--rooted", fileops.drag?.over === "" && "is-drop-root"]}
  style="flex:1"
  data-drop-root
>
  <!-- the project's row stays in view while the tree scrolls; the arrow only folds the files -->
  <div class="section-header section-header--root" oncontextmenu={onRootMenu}>
    <button
      type="button"
      class="section-header__toggle"
      aria-expanded={!ui.collapsedSections.has("vault")}
      aria-label={vault.info?.name}
      onclick={() => ui.toggleSection("vault")}
    >
      <Icon name="chevron-down" size="sm" class="twisty" />
    </button>
    <span class="folder-switch__name">{vault.info?.name}</span>
  </div>
  {#if !ui.collapsedSections.has("vault")}
    {#if filter.roots.main.length > 0 || ui.newFile?.folder === ""}
      <Tree bind:this={tree} nodes={filter.roots.main} label={t("tree.files")} root />
      {#if unread.caughtUp}
        <!-- the open document stays listed, so the empty state goes under it -->
        <p class="panel-note">{t("unread.allCaughtUp")}</p>
      {/if}
    {:else}
      <p class="panel-note">
        {unread.onlyUnread
          ? t("unread.allCaughtUp")
          : filter.hiddenFiles.length > 0
            ? t("filter.noneVisible")
            : t("explorer.empty")}
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
{#if unread.onlyUnread}
  <div class="tree-footer">
    <span class="tree-footer__text">{t("unread.onlyListed")}</span>
    <button type="button" class="link-btn" onclick={() => unread.toggleOnlyUnread()}>
      {t("filter.showAll")}
    </button>
  </div>
{:else if filter.hiddenByFilter > 0}
  <div class="tree-footer">
    <span class="tree-footer__text" title={footer}>{footer}</span>
    <button type="button" class="link-btn" onclick={() => filter.setShowAll(!filter.showAll)}>
      {filter.showAll ? t("filter.useFilter") : t("filter.showAll")}
    </button>
  </div>
{/if}

{#if fileops.drag}
  {@const drag = fileops.drag}
  <!-- follows the pointer during a drag; says what is dragged and where it would land -->
  <div
    class={["drag-chip", drag.over === null && "is-refused"]}
    style:left="{fileops.dragPoint.x + 14}px"
    style:top="{fileops.dragPoint.y + 14}px"
    role="status"
  >
    <Icon name="arrow-right-left" size="sm" />
    <span>
      {#if drag.sources.length === 1}
        {basename(drag.sources[0])}
      {:else}
        {tn("fileOps.items", drag.sources.length)}
      {/if}
      {#if drag.over !== null}
        · {t("fileOps.dropInto", {
          folder: drag.over === "" ? t("fileOps.projectRoot") : basename(drag.over),
        })}
      {/if}
    </span>
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
{#if rootMenu}
  {@const at = rootMenu}
  <Menu
    label={t("menu.fileActions")}
    anchor={{ left: at.x, right: at.x, top: at.y, bottom: at.y }}
    entries={rootEntries}
    onclose={() => (rootMenu = null)}
  />
{/if}
