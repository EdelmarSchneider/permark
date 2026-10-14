<script lang="ts">
  import { tick, untrack } from "svelte";
  import { gitStateLabel } from "../gitLabels";
  import { t } from "../i18n";
  import { isMarkdown } from "../paths";
  import { app } from "../stores/app.svelte";
  import { docs } from "../stores/docs.svelte";
  import { tags } from "../stores/tags.svelte";
  import { vault } from "../stores/vault.svelte";
  import { treeTooltip } from "../tagModel";
  import { badgeFor, flattenTree, navigate, typeahead } from "../tree";
  import type { TreeNode } from "../types";
  import Icon from "./Icon.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";

  type Props = {
    nodes: TreeNode[];
    label: string;
    /** Worktrees show no git badges in the MVP. */
    badges?: boolean;
  };
  let { nodes, label, badges = true }: Props = $props();

  let container: HTMLElement | undefined = $state();
  let focusIndex = $state(0);
  let hasFocus = $state(false);
  let menu = $state<{ x: number; y: number; node: TreeNode } | null>(null);
  let typed = "";
  let typedTimer: ReturnType<typeof setTimeout> | null = null;

  const rows = $derived(flattenTree(nodes, vault.expanded));
  const selected = $derived(docs.current?.path ?? null);

  export function focus(): void {
    container?.focus();
  }

  function rowEl(index: number): HTMLElement | null {
    return container?.querySelector<HTMLElement>(`[data-index="${index}"]`) ?? null;
  }

  async function follow(index: number): Promise<void> {
    focusIndex = index;
    await tick();
    rowEl(index)?.scrollIntoView({ block: "nearest" });
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const result = navigate(rows, focusIndex, e.key);
    if (result) {
      e.preventDefault();
      if (result.toggle) vault.toggle(result.toggle.path, result.toggle.expand);
      if (result.open) void app.openDoc(result.open);
      void follow(result.focus);
      return;
    }
    if (e.key.length === 1 && e.key !== " ") {
      typed += e.key;
      if (typedTimer !== null) clearTimeout(typedTimer);
      typedTimer = setTimeout(() => (typed = ""), 700);
      const hit = typeahead(rows, focusIndex, typed);
      if (hit !== null) void follow(hit);
    }
  }

  function activate(index: number): void {
    const row = rows[index];
    if (!row) return;
    focusIndex = index;
    if (row.isDir) vault.toggle(row.node.path);
    else void app.openDoc(row.node.path);
  }

  function entriesFor(node: TreeNode): MenuEntry[] {
    const isFile = node.kind === "file";
    return [
      {
        id: "path",
        label: t("menu.copyPath"),
        icon: "copy",
        onselect: () => void app.copyPath(node.path),
      },
      {
        id: "show",
        label: t("menu.showInFileManager"),
        icon: "folder",
        onselect: () => void app.revealInFolder(node.path),
      },
      ...(isFile
        ? ([
            {
              id: "ref",
              label: t("menu.copyReference"),
              icon: "at",
              onselect: () => void app.copyReference(node.path),
            },
            {
              id: "external",
              label: t("menu.openExternal"),
              icon: "external",
              onselect: () => void app.openExternally(node.path),
            },
          ] satisfies MenuEntry[])
        : []),
    ];
  }

  // scroll a revealed row into view (the open document, "Reveal in tree")
  $effect(() => {
    const target = vault.reveal;
    if (!target) return;
    untrack(() => {
      void tick().then(() => {
        const index = rows.findIndex((r) => r.node.path === target.path);
        if (index < 0) return;
        focusIndex = index;
        rowEl(index)?.scrollIntoView({ block: "nearest" });
      });
    });
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  bind:this={container}
  class="tree"
  role="tree"
  aria-label={label}
  tabindex="0"
  aria-activedescendant={hasFocus ? `${label}-row-${focusIndex}` : undefined}
  onfocus={() => (hasFocus = true)}
  onblur={() => (hasFocus = false)}
  {onkeydown}
>
  {#each rows as row, i (row.node.path)}
    {@const git = badges && !row.isDir ? vault.git?.files[row.node.path] : undefined}
    {@const badge = badgeFor(git?.state)}
    {@const rollup = badges && row.isDir ? vault.rollups.get(row.node.path) : undefined}
    {@const branch =
      row.node.kind === "worktree" ? vault.worktree(row.node.name)?.branch : undefined}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      id="{label}-row-{i}"
      class={[
        "tree-row",
        row.isDir && "tree-row--dir",
        selected === row.node.path && "is-selected",
        hasFocus && focusIndex === i && "is-focused",
        git?.state === "deleted" && "is-deleted",
        git?.state === "conflicted" && "is-conflicted",
      ]}
      role="treeitem"
      tabindex="-1"
      aria-expanded={row.isDir ? row.expanded : undefined}
      aria-selected={selected === row.node.path}
      aria-level={row.depth + 1}
      style:--depth={row.depth}
      data-index={i}
      data-path={row.node.path}
      title={row.node.kind === "worktree"
        ? row.node.path
        : row.isDir
          ? undefined
          : treeTooltip(row.node.path, tags.index.get(row.node.path), (list) =>
              t("tags.tooltipTags", { list }),
            )}
      onclick={() => activate(i)}
      oncontextmenu={(e) => {
        e.preventDefault();
        focusIndex = i;
        menu = { x: e.clientX, y: e.clientY, node: row.node };
      }}
    >
      {#if row.isDir}
        <Icon name="chevron-down" size="sm" class="twisty" />
      {:else}
        <span class="twisty-space"></span>
      {/if}
      {#if row.node.kind === "worktree"}
        <Icon name="fork" size="sm" />
      {:else if !row.isDir}
        <Icon name={isMarkdown(row.node.path) ? "file-text" : "file"} class="file-icon" />
      {/if}
      <span class="tree-name">{row.node.name}</span>
      {#if branch}<span class="branch-chip">{branch}</span>{/if}
      {#if rollup}
        <span
          class={[
            "rollup",
            rollup === "added" && "rollup--added",
            (rollup === "deleted" || rollup === "conflicted") && "rollup--deleted",
          ]}
          title={t("tree.containsChanges")}
        ></span>
      {:else if badge && git}
        <span class={["badge", badge.className]} title={gitStateLabel(badge.state)}
          >{badge.letter}</span
        >
      {/if}
    </div>
  {/each}
</div>

{#if menu}
  {@const at = menu}
  <Menu
    label={t("menu.fileActions")}
    anchor={{ left: at.x, right: at.x, top: at.y, bottom: at.y }}
    entries={entriesFor(at.node)}
    onclose={() => (menu = null)}
  />
{/if}
