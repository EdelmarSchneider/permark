<script lang="ts">
  import { tick, untrack } from "svelte";
  import { trackPointerDrag } from "../dom/treeDrag";
  import {
    dropFolder,
    dropProblem,
    extendSelection,
    isRootPath,
    selectAfterClick,
    targetsOf,
    topLevelOnly,
  } from "../fileOps";
  import { gitBadgeTitle } from "../gitLabels";
  import { formatNumber, t, tn } from "../i18n";
  import { categoryOf } from "../fileKinds";
  import { fileIconOf } from "../fileIcons";
  import { dirname, isWebPage } from "../paths";
  import { nearestScrollTop } from "../reveal";
  import { app } from "../stores/app.svelte";
  import { docs } from "../stores/docs.svelte";
  import { edit } from "../stores/edit.svelte";
  import { fileops } from "../stores/fileops.svelte";
  import { filter } from "../stores/filter.svelte";
  import { ui } from "../stores/ui.svelte";
  import { tags } from "../stores/tags.svelte";
  import { unread } from "../stores/unread.svelte";
  import { vault } from "../stores/vault.svelte";
  import { treeTooltip } from "../tagModel";
  import { badgeFor, flattenTree, navigate, typeahead } from "../tree";
  import type { TreeNode } from "../types";
  import Icon from "./Icon.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import TreeEntryInput from "./TreeEntryInput.svelte";
  import TreeNameInput from "./TreeNameInput.svelte";
  import UnreadDot from "./UnreadDot.svelte";

  type Props = {
    nodes: TreeNode[];
    label: string;
    /** Worktrees show no git badges in the MVP. */
    badges?: boolean;
    /** This tree holds the project's own root (a new file at the top level is typed at the top of it). */
    root?: boolean;
  };
  let { nodes, label, badges = true, root = false }: Props = $props();

  let container: HTMLElement | undefined = $state();
  let focusIndex = $state(0);
  let hasFocus = $state(false);
  let menu = $state<{ x: number; y: number; node: TreeNode } | null>(null);
  let typed = "";
  let typedTimer: ReturnType<typeof setTimeout> | null = null;
  /** A drag just ended: the click that follows the release must not open or select anything. */
  let afterDrag = false;

  const rows = $derived(flattenTree(nodes, vault.expanded));
  const rowPaths = $derived(rows.map((r) => r.node.path));
  const selected = $derived(docs.current?.path ?? null);
  const picked = $derived(fileops.selection.size > 1 ? fileops.selection : null);

  // a filter change can leave fewer rows than the focused index
  $effect(() => {
    if (focusIndex >= rows.length) focusIndex = Math.max(0, rows.length - 1);
  });

  export function focus(): void {
    container?.focus();
  }

  function rowEl(index: number): HTMLElement | null {
    return container?.querySelector<HTMLElement>(`[data-index="${index}"]`) ?? null;
  }

  /** Remembers the row the person is on: Ctrl+N puts a new file in its folder. */
  function markCursor(index: number): void {
    const row = rows[index];
    if (row) ui.treeCursor = { path: row.node.path, isDir: row.isDir };
  }

  async function follow(index: number): Promise<void> {
    focusIndex = index;
    markCursor(index);
    await tick();
    rowEl(index)?.scrollIntoView({ block: "nearest" });
  }

  /** The paths a key press acts on: the picked rows when the focused row is one of them, else the focused row. */
  function keyTargets(): string[] {
    const row = rows[focusIndex];
    return row ? targetsOf(fileops.selection, row.node.path) : [];
  }

  function onkeydown(e: KeyboardEvent): void {
    // the name field of a rename or a new folder takes its own keys
    if (e.target instanceof HTMLInputElement) return;
    const mod = e.ctrlKey || e.metaKey;
    if (mod && !e.altKey && !e.shiftKey) {
      const key = e.key.toLowerCase();
      if (key === "z") {
        e.preventDefault();
        void fileops.undo();
      } else if (key === "x" && !vault.isSimple) {
        e.preventDefault();
        fileops.cutPicked(keyTargets());
      } else if (key === "v" && fileops.cut.size > 0) {
        e.preventDefault();
        const row = rows[focusIndex];
        void fileops.paste(row ? dropFolder({ path: row.node.path, isDir: row.isDir }) : "");
      }
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey) return;

    if (e.key === "F2") {
      const row = rows[focusIndex];
      if (row) {
        e.preventDefault();
        fileops.startRename(row.node.path);
      }
      return;
    }
    if (e.key === "Delete") {
      e.preventDefault();
      void fileops.remove(keyTargets());
      return;
    }
    if (e.key === "Escape") {
      if (fileops.selection.size > 0 || fileops.cut.size > 0) {
        e.preventDefault();
        fileops.clearPicked();
      }
      return;
    }

    const result = navigate(rows, focusIndex, e.key);
    if (result) {
      e.preventDefault();
      if (e.shiftKey && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
        // Shift+Arrow extends the selection from the row it started on
        const anchor = fileops.anchor ?? rows[focusIndex]?.node.path ?? null;
        const to = rows[result.focus]?.node.path;
        if (anchor !== null && to !== undefined) {
          fileops.pick(extendSelection(rowPaths, anchor, to), anchor);
        }
        void follow(result.focus);
        return;
      }
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
    markCursor(index);
    if (row.isDir) vault.toggle(row.node.path);
    else void app.openDoc(row.node.path);
  }

  /** A click: Ctrl and Shift pick rows, a plain click opens the file or opens and closes the folder. */
  function onrowclick(e: MouseEvent, index: number): void {
    if (afterDrag) {
      afterDrag = false;
      return;
    }
    const row = rows[index];
    if (!row) return;
    const out = selectAfterClick(rowPaths, fileops.selection, fileops.anchor, row.node.path, {
      ctrl: e.ctrlKey || e.metaKey,
      shift: e.shiftKey,
    });
    fileops.pick(out.selection, out.anchor);
    if (out.plain) {
      activate(index);
    } else {
      focusIndex = index;
      markCursor(index);
    }
  }

  // ---- drag and drop -------------------------------------------------------------------------

  /** Where a drop on this element lands, or null: a row stands for its folder, the rest of the Explorer for the project. */
  function folderUnder(element: Element | null): string | null {
    const drag = fileops.drag;
    if (!drag) return null;
    const row = element?.closest<HTMLElement>("[data-path]");
    let folder: string | null = null;
    if (row) {
      folder = dropFolder({ path: row.dataset.path ?? "", isDir: row.dataset.dir === "true" });
    } else if (element?.closest("[data-drop-root]")) {
      folder = "";
    }
    return folder !== null && dropProblem(drag.sources, folder) === null ? folder : null;
  }

  function onrowpointerdown(e: PointerEvent, index: number): void {
    const row = rows[index];
    if (!row || e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return;
    if (fileops.renaming || fileops.busy || isRootPath(row.node.path)) return;
    if (e.target instanceof HTMLElement && e.target.closest("input")) return;
    const path = row.node.path;
    trackPointerDrag(e, {
      begin: () => {
        const inPicked = fileops.selection.has(path) && fileops.selection.size > 1;
        const sources = inPicked ? topLevelOnly([...fileops.selection]) : [path];
        fileops.dragPoint = { x: e.clientX, y: e.clientY };
        fileops.drag = { sources, over: null };
        afterDrag = true;
      },
      over: ({ folder, x, y }) => {
        // the rows read `drag`, which changes only when the target does; the pointer has its own state
        fileops.dragPoint = { x, y };
        if (fileops.drag && fileops.drag.over !== folder) {
          fileops.drag = { ...fileops.drag, over: folder };
        }
      },
      expand: (dir) => vault.toggle(dir, true),
      end: (folder) => {
        const drag = fileops.drag;
        fileops.drag = null;
        if (drag && folder !== null) void fileops.move(drag.sources, folder);
        // the click that follows a release comes at once; one that never comes must not swallow the next
        setTimeout(() => (afterDrag = false), 0);
      },
      folderAt: folderUnder,
      scroller: () => container?.closest<HTMLElement>(".scroll") ?? null,
    });
  }

  // ---- menu ----------------------------------------------------------------------------------

  function entriesFor(node: TreeNode): MenuEntry[] {
    const isFile = node.kind === "file";
    const targets = targetsOf(fileops.selection, node.path);
    const isRoot = isRootPath(node.path);
    const folder = isFile ? dirname(node.path) : node.path;
    return [
      ...(isFile
        ? []
        : ([
            {
              id: "openAsProject",
              label: t("menu.openAsProject"),
              icon: "folder-open",
              onselect: () => void app.placeProject(node.path),
            },
            { type: "separator" },
          ] satisfies MenuEntry[])),
      {
        id: "new",
        label: t("newFile.newFile"),
        icon: "file-plus",
        hint: "Ctrl+N",
        onselect: () => app.startNewFile(folder),
      },
      {
        id: "newFolder",
        label: t("fileOps.newFolder"),
        icon: "folder-plus",
        onselect: () => fileops.startNewFolder(folder),
      },
      { type: "separator" },
      {
        id: "rename",
        label: t("fileOps.rename"),
        icon: "pencil",
        hint: "F2",
        disabled: isRoot || targets.length > 1,
        onselect: () => fileops.startRename(node.path),
      },
      {
        id: "delete",
        label: tn("fileOps.deleteSelected", targets.length),
        icon: "trash",
        hint: "Del",
        disabled: isRoot,
        onselect: () => void fileops.remove(targets),
      },
      { type: "separator" },
      ...(isFile
        ? []
        : ([
            {
              id: "markRead",
              label: t("unread.markFolderRead"),
              disabled: unread.inFolder(node.path) === 0,
              onselect: () => void unread.markFolderRead(node.path),
            },
            { type: "separator" },
          ] satisfies MenuEntry[])),
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
            ...(isWebPage(node.path)
              ? ([
                  {
                    id: "browser",
                    label: t("menu.openInBrowser"),
                    icon: "external",
                    onselect: () => void app.openInBrowser(node.path),
                  },
                ] satisfies MenuEntry[])
              : []),
            {
              id: "external",
              label: t("menu.openExternal"),
              icon: "external",
              onselect: () => void app.openExternally(node.path),
            },
            // a document goes to the external editor; any other file can also go to the system's application
            ...(categoryOf(node.path) === "documents"
              ? []
              : ([
                  {
                    id: "defaultApp",
                    label: t("menu.openDefaultApp"),
                    icon: "external",
                    onselect: () => void app.openWithDefaultApp(node.path),
                  },
                ] satisfies MenuEntry[])),
          ] satisfies MenuEntry[])
        : []),
    ];
  }

  /**
   * Brings a row into view by the least movement (nothing moves when it is visible already). It scrolls the
   * list itself and nothing else: no smooth jump, no sideways scroll, and the keyboard focus stays put.
   */
  function showRow(index: number): void {
    const row = rowEl(index);
    const list = row?.closest<HTMLElement>(".scroll");
    if (!row || !list) return;
    const box = list.getBoundingClientRect();
    const at = row.getBoundingClientRect();
    const top = nearestScrollTop(
      { top: at.top - box.top + list.scrollTop, bottom: at.bottom - box.top + list.scrollTop },
      { scrollTop: list.scrollTop, height: list.clientHeight },
      8,
    );
    if (top !== null) list.scrollTo({ top, behavior: "instant" });
  }

  // Select the row of the open document and scroll it into view ("Always select the opened file", "Reveal open
  // doc"). It is applied even when the panel is collapsed, so opening the panel shows the file already there; when
  // another sidebar view covers the Explorer (no layout), it waits until the Explorer is back.
  let revealed = 0;
  $effect(() => {
    const target = vault.reveal;
    if (!target || ui.sidebarView !== "explorer" || target.n === revealed) return;
    untrack(() => {
      void tick().then(() => {
        const index = rows.findIndex((r) => r.node.path === target.path);
        if (index < 0) return;
        revealed = target.n;
        focusIndex = index;
        markCursor(index);
        showRow(index);
      });
    });
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  bind:this={container}
  class={["tree", fileops.drag && "is-dragging"]}
  role="tree"
  aria-label={label}
  aria-multiselectable="true"
  tabindex="0"
  aria-activedescendant={hasFocus ? `${label}-row-${focusIndex}` : undefined}
  onfocus={() => (hasFocus = true)}
  onblur={() => (hasFocus = false)}
  {onkeydown}
>
  {#if root && ui.newFile?.folder === ""}
    {#key ui.newFile.n}<TreeNameInput folder="" depth={0} />{/key}
  {/if}
  {#if root && fileops.newFolder?.folder === ""}
    {#key fileops.newFolder.n}<TreeEntryInput variant="folder" folder="" depth={0} />{/key}
  {/if}
  {#each rows as row, i (row.node.path)}
    {@const git = badges && !row.isDir ? vault.git?.files[row.node.path] : undefined}
    {@const badge = badgeFor(git?.state)}
    {@const rollup = badges && row.isDir ? vault.rollups.get(row.node.path) : undefined}
    {@const branch =
      row.node.kind === "worktree" ? vault.worktree(row.node.name)?.branch : undefined}
    {@const isPicked = picked?.has(row.node.path) === true}
    {@const renaming = fileops.renaming?.path === row.node.path}
    {@const unreadIn = row.isDir ? unread.inFolder(row.node.path) : 0}
    {@const isUnread = !row.isDir && unread.isUnread(row.node.path)}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      id="{label}-row-{i}"
      class={[
        "tree-row",
        row.isDir && "tree-row--dir",
        selected === row.node.path && "is-selected",
        isPicked && "is-picked",
        fileops.cut.has(row.node.path) && "is-cut",
        fileops.drag?.sources.includes(row.node.path) && "is-dragged",
        row.isDir && fileops.drag?.over === row.node.path && "is-drop-target",
        renaming && "tree-row--renaming",
        unreadIn > 0 && "has-unread",
        filter.forced.has(row.node.path) && "is-filtered",
        hasFocus && focusIndex === i && "is-focused",
        git?.state === "deleted" && "is-deleted",
        git?.state === "conflicted" && "is-conflicted",
      ]}
      role="treeitem"
      tabindex="-1"
      aria-expanded={row.isDir ? row.expanded : undefined}
      aria-selected={selected === row.node.path || isPicked}
      aria-level={row.depth + 1}
      style:--depth={row.depth}
      data-index={i}
      data-path={row.node.path}
      data-dir={row.isDir}
      title={renaming
        ? undefined
        : row.node.kind === "worktree"
          ? row.node.path
          : row.isDir
            ? unreadIn > 0
              ? tn("unread.folderCount", unreadIn)
              : undefined
            : filter.forced.has(row.node.path)
              ? t("filter.hiddenFile")
              : treeTooltip(row.node.path, tags.index.get(row.node.path), (list) =>
                  t("tags.tooltipTags", { list }),
                )}
      onclick={(e) => onrowclick(e, i)}
      onpointerdown={(e) => onrowpointerdown(e, i)}
      oncontextmenu={(e) => {
        e.preventDefault();
        focusIndex = i;
        markCursor(i);
        menu = { x: e.clientX, y: e.clientY, node: row.node };
      }}
    >
      {#if row.isDir}
        <Icon name="chevron-down" size="sm" class="twisty" />
      {:else}
        <span class="twisty-space"
          >{#if isUnread}<UnreadDot />{/if}</span
        >
      {/if}
      {#if row.node.kind === "worktree"}
        <Icon name="fork" size="sm" />
      {:else if !row.isDir}
        <Icon name={fileIconOf(row.node.path)} class="file-icon" />
      {/if}
      {#if unreadIn > 0}<UnreadDot label={tn("unread.folderCount", unreadIn)} />{/if}
      {#if renaming && fileops.renaming}
        {#key fileops.renaming.n}
          <TreeEntryInput variant="rename" path={row.node.path} isDir={row.isDir} />
        {/key}
      {:else}
        <span class="tree-name">{row.node.name}</span>
      {/if}
      {#if edit.dirty && edit.path === row.node.path}
        <span class="dirty-dot" role="img" aria-label={t("edit.unsaved")} title={t("edit.unsaved")}
        ></span>
      {/if}
      {#if branch}<span class="branch-chip">{branch}</span>{/if}
      {#if row.node.large !== undefined}
        <span
          class="branch-chip"
          title={t("tree.largeFolderTitle", { n: formatNumber(row.node.large) })}
          >{t("tree.largeFolder")}</span
        >
      {/if}
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
        <span
          class={["badge", badge.className]}
          title={gitBadgeTitle(badge, vault.git?.baseline.kind)}>{badge.letter}</span
        >
      {/if}
    </div>
    {#if row.isDir && row.expanded && ui.newFile?.folder === row.node.path}
      {#key ui.newFile.n}<TreeNameInput folder={row.node.path} depth={row.depth + 1} />{/key}
    {/if}
    {#if row.isDir && row.expanded && fileops.newFolder?.folder === row.node.path}
      {#key fileops.newFolder.n}
        <TreeEntryInput variant="folder" folder={row.node.path} depth={row.depth + 1} />
      {/key}
    {/if}
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
