<script lang="ts">
  // The Explorer: toolbar, a virtualized list of the project tree, the filter popover, the context menu and the move
  // dialog. All state and actions are in the store; this draws it and turns events into store calls.
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronsDownUp from "@lucide/svelte/icons/chevrons-down-up";
  import Crosshair from "@lucide/svelte/icons/crosshair";
  import FilePlus from "@lucide/svelte/icons/file-plus";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import type { TreeNode } from "../../api";
  import { conditions } from "../conditions/context.svelte";
  import { t, tn } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";
  import type { ContributionRegistry } from "../plugins/registry.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import FilterPopover from "./FilterPopover.svelte";
  import { WorktreeIcon, iconOf } from "./icons";
  import { nameOf, parentOf, topmost, type Row } from "./list";
  import type { MenuEntry } from "./menu";
  import MovePlanDialog from "./MovePlanDialog.svelte";
  import NameField from "./NameField.svelte";
  import { problemText } from "./names";
  import type { ExplorerStore } from "./store.svelte";

  type Props = {
    store: ExplorerStore;
    /** Plugins add badges to rows through this registry. */
    registry?: ContributionRegistry;
  };
  let { store, registry = contributions }: Props = $props();

  /** Row height (px): fixed, so the list is drawn from arithmetic. */
  const ROW = 26;
  /** Rows drawn beyond the visible ones. */
  const OVERSCAN = 8;
  /** Pointer travel (px) before a press becomes a drag. */
  const DRAG_START = 4;
  const EDGE = 28;

  type Display = Row | { type: "new"; depth: number; dir: string };
  type Press = { x: number; y: number; paths: readonly string[] };

  let list: HTMLElement | undefined = $state();
  let scrollTop = $state(0);
  let viewport = $state(400);
  let filterOpen = $state(false);
  let menu = $state.raw<{ x: number; y: number; entries: readonly MenuEntry[] } | null>(null);
  let press: Press | null = null;
  let stopPress: (() => void) | null = null;
  let suppressClick = false;
  let handledScroll = 0;

  const badges = $derived(registry.visible("treeBadge", conditions.current));

  /** The rows, with the name field of a new entry right after its folder. */
  const display = $derived.by((): Display[] => {
    const rows: Display[] = store.rows;
    const edit = store.edit;
    if (edit.mode !== "new") return rows;
    const at =
      edit.dir === ""
        ? 0
        : store.rows.findIndex((r) => r.type === "node" && r.node.path === edit.dir) + 1;
    const dirRow = store.rows[at - 1];
    const depth = edit.dir === "" ? 0 : dirRow?.type === "node" ? dirRow.depth + 1 : 0;
    return [...rows.slice(0, at), { type: "new", depth, dir: edit.dir }, ...rows.slice(at)];
  });

  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const last = $derived(
    Math.min(display.length, Math.ceil((scrollTop + viewport) / ROW) + OVERSCAN),
  );
  const slice = $derived(display.slice(first, last));

  const countText = $derived(
    store.shownFiles === store.totalFiles
      ? tn("filter.showingAll", store.totalFiles)
      : tn("filter.showingOf", store.totalFiles, { shown: store.shownFiles }),
  );

  // viewport height follows the panel (observer torn down with the list)
  $effect(() => {
    if (!list || typeof ResizeObserver === "undefined") return;
    const el = list;
    const measure = (): void => {
      if (el.clientHeight > 0) viewport = el.clientHeight;
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  });

  // a drag in progress ends with the panel
  $effect(() => () => stopPress?.());

  // a request to bring a row into view (reveal, keyboard moves, a new field)
  $effect(() => {
    const request = store.scrollTo;
    if (!request || !list || request.n === handledScroll) return;
    const at = display.findIndex((r) => r.type === "node" && r.node.path === request.path);
    if (at < 0) return;
    handledScroll = request.n;
    const top = at * ROW;
    if (top < list.scrollTop) list.scrollTop = top;
    else if (top + ROW > list.scrollTop + list.clientHeight) {
      list.scrollTop = top + ROW - list.clientHeight;
    }
  });

  function badgeText(path: string): string[] {
    return badges.flatMap((b) => {
      const text = b.badge(path);
      return text === null ? [] : [text];
    });
  }

  function rowPath(row: Display): string {
    return row.type === "node" ? row.node.path : row.type === "root" ? row.root.path : "";
  }

  // ---- mouse and keyboard on rows ----------------------------------------------------------------------------------

  function onrowclick(e: MouseEvent, node: TreeNode): void {
    if (suppressClick) {
      suppressClick = false;
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      store.select(node.path, "toggle");
    } else if (e.shiftKey) {
      store.select(node.path, "range");
    } else {
      store.select(node.path, "single");
      void store.activate(node.path);
    }
  }

  function oncontext(e: MouseEvent, node: TreeNode): void {
    e.preventDefault();
    if (!store.selection.has(node.path)) store.select(node.path, "single");
    menu = { x: e.clientX, y: e.clientY, entries: entriesFor(node) };
  }

  function entriesFor(node: TreeNode): MenuEntry[] {
    const targets = store.targets();
    const many = targets.length > 1;
    const isDir = node.kind.type === "dir";
    const category = node.kind.type === "file" ? node.kind.category : null;
    const entries: MenuEntry[] = [];
    if (!many) {
      if (
        !isDir &&
        category !== null &&
        category !== "other" &&
        !node.name.toLowerCase().endsWith(".svg")
      ) {
        entries.push({
          kind: "item",
          label: t("menu.openDefaultApp"),
          run: () => void store.openExternal(node.path, "defaultApp"),
        });
      }
      if (/\.html?$/i.test(node.name)) {
        entries.push({
          kind: "item",
          label: t("menu.openInBrowser"),
          run: () => void store.openExternal(node.path, "browser"),
        });
      }
      if (isDir && node.kind.type === "dir" && node.kind.unlisted === null) {
        entries.push({
          kind: "item",
          label: t("menu.openAsProject"),
          run: () => void store.openAsProject(node.path),
        });
      }
      if (!isDir) {
        entries.push({
          kind: "item",
          label: t("header.openExternal"),
          run: () => void store.openInEditor(node.path),
        });
      }
      entries.push(
        {
          kind: "item",
          label: t("menu.showInFileManager"),
          run: () => void store.openExternal(node.path, "reveal"),
        },
        { kind: "separator" },
        {
          kind: "item",
          label: t("menu.copyPath"),
          run: () => void store.copyPath(node.path, false),
        },
        {
          kind: "item",
          label: t("menu.copyFullPath"),
          run: () => void store.copyPath(node.path, true),
        },
        {
          kind: "item",
          label: t("header.copyReference"),
          run: () => void store.copyReference(node.path),
        },
        { kind: "separator" },
        {
          kind: "item",
          label: t("newFile.newFile"),
          run: () => store.beginNew("file", isDir ? node.path : parentOf(node.path)),
        },
        {
          kind: "item",
          label: t("fileOps.newFolder"),
          run: () => store.beginNew("folder", isDir ? node.path : parentOf(node.path)),
        },
        { kind: "separator" },
        {
          kind: "item",
          label: t("fileOps.rename"),
          hint: "F2",
          run: () => store.beginRename(node.path),
        },
      );
    }
    entries.push({
      kind: "item",
      label: many ? tn("fileOps.deleteSelected", targets.length) : t("fileOps.delete"),
      hint: "Del",
      danger: true,
      run: () => void store.remove(),
    });
    return entries;
  }

  function onkeydown(e: KeyboardEvent): void {
    // typing in the name field is the field's business; dead keys and IME never fire shortcuts
    if (e.isComposing || e.target instanceof HTMLInputElement) return;
    const modifier = e.ctrlKey || e.metaKey;
    let handled = true;
    switch (e.key) {
      case "ArrowDown":
        store.move(1, e.shiftKey);
        break;
      case "ArrowUp":
        store.move(-1, e.shiftKey);
        break;
      case "ArrowRight":
        void store.arrow("right");
        break;
      case "ArrowLeft":
        void store.arrow("left");
        break;
      case "Home":
        store.moveTo("first", e.shiftKey);
        break;
      case "End":
        store.moveTo("last", e.shiftKey);
        break;
      case "Enter":
        if (store.focus !== null) void store.activate(store.focus);
        break;
      case "F2":
        store.beginRename();
        break;
      case "Delete":
        void store.remove();
        break;
      case "Escape":
        if (store.drag.status === "dragging") store.cancelDrag();
        else handled = false;
        break;
      case "z":
      case "Z":
        if (modifier && !e.shiftKey) void store.undo();
        else handled = false;
        break;
      default:
        handled = false;
    }
    if (handled) e.preventDefault();
  }

  // ---- drag ------------------------------------------------------------------------------------------------------------

  function onpointerdown(e: PointerEvent, node: TreeNode): void {
    if (e.button !== 0 || e.target instanceof HTMLInputElement || !list) return;
    stopPress?.();
    const paths = store.selection.has(node.path) ? topmost(store.selection) : [node.path];
    press = { x: e.clientX, y: e.clientY, paths };
    // listeners on the window (not pointer capture, which would move the click away from the row); gone at the end
    const up = (): void => {
      stopPress?.();
      if (store.drag.status === "dragging") void store.endDrag();
    };
    const cancel = (): void => {
      stopPress?.();
      store.cancelDrag();
    };
    window.addEventListener("pointermove", onpointermove);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    stopPress = () => {
      window.removeEventListener("pointermove", onpointermove);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      press = null;
      stopPress = null;
    };
  }

  /** Folder a pointer position would drop into (`""` = the project folder), or null outside the list. */
  function targetAt(x: number, y: number): string | null {
    if (!list) return null;
    const box = list.getBoundingClientRect();
    if (x < box.left || x > box.right || y < box.top || y > box.bottom) return null;
    const row = display[Math.floor((y - box.top + list.scrollTop) / ROW)];
    if (row === undefined) return "";
    switch (row.type) {
      case "root":
        return row.root.path;
      case "node":
        return row.node.kind.type === "dir" && row.node.kind.unlisted === null
          ? row.node.path
          : parentOf(row.node.path);
      case "new":
        return null;
      default:
        return row satisfies never;
    }
  }

  function onpointermove(e: PointerEvent): void {
    if (!press || !list) return;
    if (store.drag.status === "idle") {
      if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_START) return;
      store.beginDrag(press.paths);
      suppressClick = true;
    }
    store.dragOver(targetAt(e.clientX, e.clientY));
    // near an edge the list scrolls
    const box = list.getBoundingClientRect();
    if (e.clientY < box.top + EDGE) list.scrollTop -= 12;
    else if (e.clientY > box.bottom - EDGE) list.scrollTop += 12;
  }
</script>

<div class="explorer">
  <div class="bar">
    <span class="menu-label">{t("project.explorer")}</span>
    <span class="spacer"></span>
    <button
      type="button"
      class="icon"
      title={t("newFile.newFile")}
      aria-label={t("newFile.newFile")}
      onclick={() => store.beginNew("file")}
    >
      <FilePlus size={15} />
    </button>
    <button
      type="button"
      class="icon"
      title={t("fileOps.newFolder")}
      aria-label={t("fileOps.newFolder")}
      onclick={() => store.beginNew("folder")}
    >
      <FolderPlus size={15} />
    </button>
    <button
      type="button"
      class="icon"
      title={t("explorer.revealOpen")}
      aria-label={t("explorer.revealOpen")}
      onclick={() => store.revealActive()}
    >
      <Crosshair size={15} />
    </button>
    <button
      type="button"
      class="icon"
      title={t("explorer.collapseAll")}
      aria-label={t("explorer.collapseAll")}
      onclick={() => store.collapseAll()}
    >
      <ChevronsDownUp size={15} />
    </button>
    <button
      type="button"
      class={["icon", filterOpen && "is-on"]}
      data-filter-trigger
      title={t("filter.title")}
      aria-label={t("filter.title")}
      aria-expanded={filterOpen}
      onclick={() => (filterOpen = !filterOpen)}
    >
      <ListFilter size={15} />
    </button>
  </div>

  {#if filterOpen}
    <FilterPopover {store} close={() => (filterOpen = false)} />
  {/if}

  {#if store.load.status === "loading"}
    <p class="note" role="status">{t("explorer.loading")}</p>
  {:else if store.load.status === "failed"}
    <div class="note" role="alert">
      <p>{t("explorer.failed")}</p>
      <button type="button" class="btn btn--sm" onclick={() => void store.retry()}>
        {t("explorer.retry")}
      </button>
    </div>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      bind:this={list}
      class={["list", store.drag.status === "dragging" && store.drag.over === "" && "drop-root"]}
      role="tree"
      aria-label={t("project.explorer")}
      aria-multiselectable="true"
      tabindex="0"
      onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
      {onkeydown}
    >
      {#if display.length === 0}
        <p class="note">{t("explorer.empty")}</p>
      {/if}
      <div class="space" style:height="{display.length * ROW}px">
        {#each slice as row, at (rowPath(row) + row.type)}
          {@const top = (first + at) * ROW}
          {#if row.type === "root"}
            <button
              type="button"
              class="row root"
              style:top="{top}px"
              style:height="{ROW}px"
              aria-expanded={row.expanded}
              onclick={() => store.toggleRoot(row.root.path)}
            >
              {#if row.expanded}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
              <WorktreeIcon size={15} />
              <span class="name">{row.root.name}</span>
            </button>
          {:else if row.type === "new"}
            <div
              class="row"
              style:top="{top}px"
              style:height="{ROW}px"
              style:padding-left="{8 + row.depth * 14 + 18}px"
            >
              <NameField
                value={store.edit.mode === "new" ? store.edit.value : ""}
                label={store.edit.mode === "new" && store.edit.kind === "folder"
                  ? t("fileOps.newFolderLabel")
                  : t("newFile.label")}
                hint={store.edit.mode === "new" && store.edit.kind === "folder"
                  ? t("fileOps.newFolderHint")
                  : t("newFile.hint")}
                problem={store.edit.mode === "new" && store.edit.problem !== null
                  ? problemText(store.edit.problem)
                  : null}
                oninput={(v) => store.typeName(v)}
                commit={() => void store.commitEdit()}
                cancel={() => store.cancelEdit()}
              />
            </div>
          {:else}
            {@const node = row.node}
            {@const Icon = iconOf(node, row.expanded)}
            {@const isDir = node.kind.type === "dir"}
            {@const selected = store.selection.has(node.path)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class={[
                "row",
                selected && "is-selected",
                store.focus === node.path && "is-focused",
                (node.hidden || node.ignored) && "is-dim",
                node.forced && "is-forced",
                store.drag.status === "dragging" && store.drag.over === node.path && "drop-target",
              ]}
              role="treeitem"
              tabindex="-1"
              aria-selected={selected}
              aria-level={row.depth + 1}
              aria-expanded={isDir ? row.expanded : undefined}
              data-path={node.path}
              style:top="{top}px"
              style:height="{ROW}px"
              style:padding-left="{8 + row.depth * 14}px"
              title={node.forced ? t("filter.hiddenFile") : undefined}
              onclick={(e) => onrowclick(e, node)}
              oncontextmenu={(e) => oncontext(e, node)}
              onpointerdown={(e) => onpointerdown(e, node)}
            >
              {#if isDir}
                <button
                  type="button"
                  class="chev"
                  tabindex="-1"
                  aria-label={row.expanded ? t("explorer.collapse") : t("explorer.expand")}
                  onclick={(e) => {
                    e.stopPropagation();
                    void store.toggle(node.path);
                  }}
                  onpointerdown={(e) => e.stopPropagation()}
                >
                  {#if row.expanded}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
                </button>
              {:else}
                <span class="chev"></span>
              {/if}
              <span class="icon-cell"><Icon size={15} /></span>
              {#if store.edit.mode === "rename" && store.edit.path === node.path}
                <NameField
                  value={store.edit.value}
                  label={t("fileOps.renameLabel", { name: node.name })}
                  hint={t("fileOps.renameHint")}
                  problem={store.edit.problem === null ? null : problemText(store.edit.problem)}
                  oninput={(v) => store.typeName(v)}
                  commit={() => void store.commitEdit()}
                  cancel={() => store.cancelEdit()}
                />
              {:else}
                <span class="name">{nameOf(node.path)}</span>
                {#if node.kind.type === "dir" && node.kind.unlisted?.type === "large"}
                  <span class="tag" title={t("tree.largeFolderTitle", { n: 5000 })}>
                    {t("tree.largeFolder")}
                  </span>
                {:else if node.kind.type === "dir" && node.kind.unlisted?.type === "ignored"}
                  <span class="tag" title={t("explorer.ignoredHint")}>{t("explorer.ignored")}</span>
                {/if}
                {#each badgeText(node.path) as badge, n (n)}
                  <span class="badge">{badge}</span>
                {/each}
              {/if}
            </div>
          {/if}
        {/each}
      </div>
    </div>
    <div class="foot" role="status">
      <span class="count">{countText}</span>
      {#if store.shownFiles !== store.totalFiles}
        <button
          type="button"
          class="link"
          onclick={() => void store.setFilter({ ...store.filter, show: "everything" })}
        >
          {t("filter.showAll")}
        </button>
      {/if}
    </div>
  {/if}
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} close={() => (menu = null)} />
{/if}
{#if store.dialog.status === "asking"}
  <MovePlanDialog plan={store.dialog.plan} answer={(choice) => void store.answerDialog(choice)} />
{/if}
{#if store.drag.status === "dragging" && store.drag.over !== null}
  <div class="drop-hint" aria-hidden="true">
    {t("fileOps.dropInto", {
      folder: store.drag.over === "" ? t("fileOps.projectRoot") : nameOf(store.drag.over),
    })}
  </div>
{/if}

<style>
  .explorer {
    position: relative;
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 var(--sp-2);
    min-height: 32px;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover,
  .icon.is-on {
    background: var(--hover);
    color: var(--text);
  }
  .list {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: auto;
    outline: none;
    user-select: none;
  }
  .list:focus-visible {
    box-shadow: inset 0 0 0 1px var(--accent-muted);
  }
  .list.drop-root {
    background: var(--accent-soft);
  }
  .space {
    position: relative;
  }
  .row {
    position: absolute;
    left: 0;
    right: 0;
    display: flex;
    align-items: center;
    gap: 4px;
    padding-right: var(--sp-2);
    color: var(--text);
    font-size: var(--fs-ui);
    cursor: default;
  }
  button.row {
    width: 100%;
    border: 0;
    background: transparent;
    font: inherit;
    font-size: var(--fs-ui);
    text-align: left;
    padding-left: var(--sp-2);
    color: var(--muted);
    cursor: pointer;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.is-selected {
    background: var(--accent-soft);
  }
  .row.is-focused {
    box-shadow: inset 0 0 0 1px var(--accent-muted);
  }
  .row.is-dim {
    color: var(--faint);
  }
  .row.is-forced .name {
    font-style: italic;
  }
  .row.drop-target {
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .chev {
    display: grid;
    place-items: center;
    flex: none;
    width: 14px;
    height: 20px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--faint);
  }
  .icon-cell {
    display: grid;
    place-items: center;
    flex: none;
    color: var(--muted);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag,
  .badge {
    flex: none;
    padding: 0 5px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .note {
    margin: 0;
    padding: var(--sp-3) var(--sp-4);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: 4px var(--sp-3);
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .link {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--accent-text);
    font: inherit;
    cursor: pointer;
  }
  .drop-hint {
    position: fixed;
    left: var(--sp-3);
    bottom: calc(var(--h-status) + var(--sp-2));
    z-index: 40;
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--bg);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    pointer-events: none;
    box-shadow: var(--shadow-pop);
  }
</style>
