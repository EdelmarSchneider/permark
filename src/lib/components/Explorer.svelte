<script lang="ts">
  import { untrack } from "svelte";
  import { t, tn } from "../i18n";
  import { app } from "../stores/app.svelte";
  import { docs } from "../stores/docs.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import Tree from "./Tree.svelte";

  let tree: Tree | undefined = $state();
  let menuAnchor = $state<DOMRect | null>(null);

  const projectOpen = $derived(vault.stateLocation === "project");
  const entries = $derived<MenuEntry[]>([
    { id: "collapse", label: t("explorer.collapseAll"), onselect: () => vault.collapseAll() },
    {
      id: "reveal",
      label: t("explorer.revealOpen"),
      disabled: !docs.current,
      onselect: () => docs.path && vault.revealPath(docs.path),
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

  $effect(() => {
    void ui.explorerFocusTick;
    untrack(() => {
      if (ui.explorerFocusTick > 0) tree?.focus();
    });
  });

  function toggleMenu(e: MouseEvent): void {
    menuAnchor = menuAnchor ? null : (e.currentTarget as HTMLElement).getBoundingClientRect();
  }
</script>

<div class="panel-header">
  <span class="panel-title">{t("explorer.title")}</span>
  <span class="spacer"></span>
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
    {#if vault.roots.main.length > 0}
      <Tree bind:this={tree} nodes={vault.roots.main} label={t("tree.files")} />
    {:else}
      <p class="panel-note">{t("explorer.empty")}</p>
    {/if}
  {/if}

  {#if vault.roots.worktrees.length > 0}
    <button
      class="section-header"
      aria-expanded={!ui.collapsedSections.has("worktrees")}
      onclick={() => ui.toggleSection("worktrees")}
    >
      <Icon name="chevron-down" size="sm" class="twisty" />{t("explorer.worktrees")}
      <span
        class="section-header__aside"
        title={tn("explorer.worktreeCount", vault.roots.worktrees.length)}
      >
        {vault.roots.worktrees.length}
      </span>
    </button>
    {#if !ui.collapsedSections.has("worktrees")}
      <Tree nodes={vault.roots.worktrees} label={t("tree.worktrees")} badges={false} />
    {/if}
  {/if}
</div>

{#if menuAnchor}
  <Menu
    label={t("explorer.menu")}
    anchor={menuAnchor}
    align="end"
    {entries}
    onclose={() => (menuAnchor = null)}
  />
{/if}
