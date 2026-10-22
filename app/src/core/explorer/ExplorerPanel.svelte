<script lang="ts">
  // The Explorer: toolbar, a virtualized list of the project tree, the filter popover, the context menu and the move
  // dialog. All state and actions are in the store; this draws it and turns events into store calls. Included projects
  // inside the folder get their own look, a detail card and menu entries; their question sits under the list, then the
  // code offer, then a plugin's note (one strip at a time).
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronsDownUp from "@lucide/svelte/icons/chevrons-down-up";
  import Crosshair from "@lucide/svelte/icons/crosshair";
  import FilePlus from "@lucide/svelte/icons/file-plus";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import GitCommitHorizontal from "@lucide/svelte/icons/git-commit-horizontal";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import type { ProjectRow, TreeNode } from "../../api";
  import { conditions } from "../conditions/context.svelte";
  import { t, tn } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";
  import type { ContributionRegistry, ProjectFact, TreeBadge } from "../plugins/registry.svelte";
  import { projects } from "../projects/instance.svelte";
  import { aliasOf, hueOfMember } from "../projects/model";
  import ProjectStrip from "../projects/ProjectStrip.svelte";
  import Badge from "../shell/Badge.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import FilterPopover from "./FilterPopover.svelte";
  import { WorktreeIcon, iconOf, projectIcon } from "./icons";
  import { EdgeScroll } from "./edgeScroll";
  import { dropBlock, nameOf, parentOf, rowKey, topmost, type Row } from "./list";
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
  /** A pointer resting this long on a project node shows its card. */
  const CARD_HOVER_MS = 500;
  /** Grace after the pointer leaves the node | the card before the card goes (time to move into it). */
  const CARD_LEAVE_MS = 300;

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
  /** What plugins know about included projects (git: the branch after the node's name). */
  const facts = $derived(registry.visible("projectFacts", conditions.current));
  /** The plugins' strip under the list (the offer to share with agents): the first that has something to say. */
  const note = $derived(
    registry
      .visible("explorerNote", conditions.current)
      .sort((a, b) => a.order - b.order)
      .find((n) => n.when()) ?? null,
  );
  /** The project the detail card is about (gone with the project). */
  const cardRow = $derived(projects.card === null ? null : projects.memberAt(projects.card.path));

  /** The rows that read as one block with the folder an outside drag is over (its visible contents). */
  const block = $derived.by((): ReadonlySet<string> => {
    const range = dropBlock(store.rows, store.outside);
    return range === null
      ? new Set()
      : new Set(store.rows.slice(range.first, range.last + 1).map(rowKey));
  });

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

  // near the top | bottom edge a drag scrolls the list (the frame loop goes with the list)
  let edge: EdgeScroll | null = null;
  $effect(() => {
    if (!list) return;
    const scroller = new EdgeScroll(list);
    edge = scroller;
    return () => {
      scroller.stop();
      if (edge === scroller) edge = null;
    };
  });
  // a drag from outside: its pointer
  $effect(() => {
    const point = store.outsidePoint;
    if (store.drag.status !== "dragging") edge?.point(point?.y ?? null);
  });

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

  /** What plugins say about a row (a git letter, an unread dot or count). */
  function badgesOf(node: TreeNode): TreeBadge[] {
    const isDir = node.kind.type === "dir";
    return badges.flatMap((b) => {
      const badge = b.badge(node.path, isDir);
      return badge === null ? [] : [badge];
    });
  }

  /** The tooltip of a row: why it is shown although hidden, and what plugins add (tags, silence). */
  function rowTitle(node: TreeNode, said: readonly TreeBadge[]): string | undefined {
    const lines = [
      ...(node.forced ? [t("filter.hiddenFile")] : []),
      ...said.flatMap((b) => (b.tip === undefined ? [] : [b.tip])),
    ];
    return lines.length === 0 ? undefined : lines.join("\n");
  }

  /** The first fact a plugin gives for a project node (its branch | what its repository is doing). */
  function factOf(member: ProjectRow): ProjectFact | null {
    for (const contribution of facts) {
      const fact = contribution.node(member);
      if (fact !== null) return fact;
    }
    return null;
  }

  /** What a screen reader hears for a project node: the words it shows first (folder name, short name when that
   *  differs, branch | state; WCAG 2.5.3), "project", what the fact adds ("branch develop", a problem), what plugins
   *  say (changed files, unread). */
  function projectName(
    node: TreeNode,
    member: ProjectRow,
    fact: ProjectFact | null,
    said: readonly TreeBadge[],
  ): string {
    const shown = [nameOf(node.path), aliasOf(member) ?? "", fact?.text ?? ""]
      .filter((words) => words !== "")
      .join(" ");
    return [
      t("projects.node.sr", { folder: shown }),
      ...(fact?.spoken === undefined ? [] : [fact.spoken]),
      ...said.flatMap((b) => (b.title === undefined || b.text === "" ? [] : [b.title])),
    ].join(", ");
  }

  // the project nodes open on screen: plugins fetch their facts for these (git: the changed files inside)
  $effect(() => {
    const open = slice.flatMap((row) =>
      row.type === "node" && row.expanded && row.node.kind.type === "dir"
        ? (projects.memberAt(row.node.path)?.uid ?? [])
        : [],
    );
    projects.look("explorer", "glance", open);
  });
  $effect(() => () => projects.look("explorer", "glance", []));

  // ---- the detail card of a project node ---------------------------------------------------------------------------

  let hoverTimer: ReturnType<typeof setTimeout> | null = null;
  let leaveTimer: ReturnType<typeof setTimeout> | null = null;

  function clearCardTimers(): void {
    if (hoverTimer !== null) clearTimeout(hoverTimer);
    if (leaveTimer !== null) clearTimeout(leaveTimer);
    hoverTimer = null;
    leaveTimer = null;
  }
  // timers and the card go with the panel
  $effect(() => () => {
    clearCardTimers();
    projects.closeCard();
  });

  function rowElement(path: string): HTMLElement | null {
    return list?.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`) ?? null;
  }

  function onrowenter(node: TreeNode, member: ProjectRow | null): void {
    if (member === null || store.drag.status === "dragging") return;
    if (leaveTimer !== null) clearTimeout(leaveTimer);
    leaveTimer = null;
    if (projects.card?.path === node.path) return;
    if (hoverTimer !== null) clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => {
      hoverTimer = null;
      const el = rowElement(node.path);
      if (el && menu === null) {
        projects.openCard({ path: node.path, from: "hover", anchor: el.getBoundingClientRect() });
      }
    }, CARD_HOVER_MS);
  }

  function onrowleave(): void {
    if (hoverTimer !== null) clearTimeout(hoverTimer);
    hoverTimer = null;
    cardHovering(false);
  }

  /** The pointer is over the card (it stays) | left it (it goes after a moment; a card opened from the menu stays). */
  function cardHovering(inside: boolean): void {
    if (leaveTimer !== null) clearTimeout(leaveTimer);
    leaveTimer = null;
    if (inside || projects.card?.from !== "hover") return;
    leaveTimer = setTimeout(() => {
      leaveTimer = null;
      if (projects.card?.from === "hover") projects.closeCard();
    }, CARD_LEAVE_MS);
  }

  function showCard(path: string): void {
    clearCardTimers();
    const el = rowElement(path);
    if (el) projects.openCard({ path, from: "menu", anchor: el.getBoundingClientRect() });
  }

  const projectCard = (): Promise<typeof import("../projects/ProjectCard.svelte")> =>
    import("../projects/ProjectCard.svelte");

  /** The card's "Open in new tab": what it is about is read before it closes. */
  function cardOpenTab(): void {
    const path = projects.card?.path ?? null;
    closeCard(false);
    if (path !== null) void store.openAsProject(path);
  }

  /** The card's "Project settings…": that project's row in Settings, opened and focused. */
  function cardSettings(): void {
    const row = cardRow;
    closeCard(false);
    if (row !== null) projects.openSettings(row);
  }

  function closeCard(refocus: boolean): void {
    clearCardTimers();
    const path = projects.card?.path ?? null;
    projects.closeCard();
    if (refocus && path !== null) {
      store.focus = path;
      list?.focus();
    }
  }

  /** The entries plugins add to the menu of one row. */
  function pluginEntries(node: TreeNode, targets: readonly string[]): MenuEntry[] {
    const isDir = node.kind.type === "dir";
    return registry
      .visible("treeMenu", conditions.current)
      .filter((entry) => entry.applies(node.path, isDir))
      .map((entry) => ({
        kind: "item" as const,
        label: t(entry.label),
        run: () => entry.run(targets),
      }));
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

  /** A click on the empty part of the tree: what the project as a whole can do. */
  function onbackground(e: MouseEvent): void {
    if (!(e.target instanceof Element) || e.target.closest(".row")) return;
    e.preventDefault();
    const entries: MenuEntry[] = [
      { kind: "item", label: t("newFile.newFile"), run: () => store.beginNew("file") },
      { kind: "item", label: t("fileOps.newFolder"), run: () => store.beginNew("folder") },
    ];
    if (conditions.current.permark === "notInitialized") {
      entries.push(
        { kind: "separator" },
        {
          kind: "item",
          label: t("menu.initProject"),
          run: () => void store.askInit("", store.roots[0]?.name ?? ""),
        },
      );
    }
    menu = { x: e.clientX, y: e.clientY, entries };
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
        category !== "code" &&
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
      const member = isDir ? projects.memberAt(node.path) : null;
      if (isDir && node.kind.type === "dir" && node.kind.unlisted === null) {
        entries.push({
          kind: "item",
          // a project: the same words as its card's button (one name per action)
          label: member === null ? t("menu.openAsProject") : t("projects.card.openTab"),
          run: () => void store.openAsProject(node.path),
        });
      }
      if (member !== null) {
        entries.push(
          { kind: "item", label: t("projects.menu.details"), run: () => showCard(node.path) },
          {
            kind: "item",
            label: t("projects.menu.settings"),
            run: () => projects.openSettings(member),
          },
        );
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
        ...withSeparator(pluginEntries(node, targets)),
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

  /** `entries` set apart by a separator, when there are any. */
  function withSeparator(entries: MenuEntry[]): MenuEntry[] {
    return entries.length === 0 ? [] : [{ kind: "separator" }, ...entries];
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
      case "ContextMenu":
        openMenuAtFocus();
        break;
      case "F10":
        if (e.shiftKey) openMenuAtFocus();
        else handled = false;
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

  /** The Menu key | Shift+F10: the menu of the row the keyboard is on, at that row. */
  function openMenuAtFocus(): void {
    const path = store.focus;
    if (path === null) return;
    const node = store.nodes.find((n) => n.path === path);
    const el = rowElement(path);
    if (!node || !el) return;
    if (!store.selection.has(path)) store.select(path, "single");
    const box = el.getBoundingClientRect();
    menu = { x: box.left + 24, y: box.bottom, entries: entriesFor(node) };
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
      edge?.point(null);
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

  /** Folder files dragged in from outside would land in at a point: any folder row is itself (also an unopened large
   *  | ignored one), a file row its folder, the empty part the project folder; `null` outside the list. */
  function importFolderAt(x: number, y: number): string | null {
    if (!list) return null;
    const box = list.getBoundingClientRect();
    if (x < box.left || x > box.right || y < box.top || y > box.bottom) return null;
    const row = display[Math.floor((y - box.top + list.scrollTop) / ROW)];
    if (row === undefined || row.type === "new") return "";
    return row.type === "root"
      ? row.root.path
      : row.node.kind.type === "dir"
        ? row.node.path
        : parentOf(row.node.path);
  }

  // the window can ask where a drop from outside would land (it draws the mark through the store)
  $effect(() => {
    store.geometry = importFolderAt;
    return () => {
      store.geometry = null;
    };
  });

  function onpointermove(e: PointerEvent): void {
    if (!press || !list) return;
    if (store.drag.status === "idle") {
      if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_START) return;
      store.beginDrag(press.paths);
      suppressClick = true;
    }
    store.dragOver(targetAt(e.clientX, e.clientY));
    edge?.point(e.clientY);
  }
</script>

<div class="explorer" data-drop-explorer>
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
      class={[
        "list",
        ((store.drag.status === "dragging" && store.drag.over === "") || store.outside === "") &&
          "drop-root",
      ]}
      role="tree"
      aria-label={t("project.explorer")}
      aria-multiselectable="true"
      tabindex="0"
      onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
      oncontextmenu={onbackground}
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
              class={[
                "row root",
                store.outside === row.root.path && "drop-target",
                block.has(row.root.path) && "drop-block",
              ]}
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
                note={store.edit.mode === "new" && store.edit.kind === "file"
                  ? t("newFile.note")
                  : null}
                oninput={(v) => store.typeName(v)}
                commit={() => void store.commitEdit()}
                cancel={() => store.cancelEdit()}
              />
            </div>
          {:else}
            {@const node = row.node}
            {@const isDir = node.kind.type === "dir"}
            {@const member = isDir ? projects.memberAt(node.path) : null}
            {@const Icon =
              member === null
                ? iconOf(node, row.expanded || store.outside === node.path)
                : projectIcon(member.marker)}
            {@const selected = store.selection.has(node.path)}
            {@const said = badgesOf(node)}
            {@const fact = member === null ? null : factOf(member)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class={[
                "row",
                selected && "is-selected",
                store.focus === node.path && "is-focused",
                (node.hidden || node.ignored || said.some((b) => b.dim)) && "is-dim",
                node.forced && "is-forced",
                ((store.drag.status === "dragging" && store.drag.over === node.path) ||
                  store.outside === node.path) &&
                  "drop-target",
                block.has(node.path) && "drop-block",
              ]}
              role="treeitem"
              tabindex="-1"
              aria-selected={selected}
              aria-level={row.depth + 1}
              aria-expanded={isDir ? row.expanded : undefined}
              aria-label={member === null ? undefined : projectName(node, member, fact, said)}
              data-path={node.path}
              data-project={member === null ? undefined : ""}
              style:top="{top}px"
              style:height="{ROW}px"
              style:padding-left="{8 + row.depth * 14}px"
              title={member === null ? rowTitle(node, said) : undefined}
              onclick={(e) => onrowclick(e, node)}
              oncontextmenu={(e) => oncontext(e, node)}
              onpointerdown={(e) => onpointerdown(e, node)}
              onpointerenter={() => onrowenter(node, member)}
              onpointerleave={onrowleave}
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
              {#if member === null}
                <span class="icon-cell"><Icon size={15} /></span>
              {:else}
                <span
                  class="icon-cell project"
                  style:--hue="var(--tag-{hueOfMember(projects.root ?? '', member)})"
                  ><Icon size={15} /></span
                >
              {/if}
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
                {@const alias = member === null ? null : aliasOf(member)}
                {#if alias === null}
                  <span class={["name", fact !== null && "with-fact"]}>{nameOf(node.path)}</span>
                {:else}
                  <!-- the folder name first (what paths say); the project's short name after it, muted, cut first -->
                  <span class={["name named", fact !== null && "with-fact"]}
                    ><span class="folder">{nameOf(node.path)}</span><span class="alias"
                      >{alias}</span
                    ></span
                  >
                {/if}
                {#if fact !== null}
                  <!-- the branch | what the repository is doing: muted, cut before the name, gone in a narrow sidebar;
                       a state chip is never cut (the name gives way); a problem: a warning mark first; a branch: a
                       branch mark first (reads apart from the short name) -->
                  <span
                    class={[
                      "fact",
                      fact.chip && "chip",
                      fact.warn && "warn",
                      (fact.branch || fact.commit) && "marked",
                    ]}
                    title={fact.title ?? fact.text}
                    aria-hidden="true"
                    >{#if fact.warn}<TriangleAlert
                        size={12}
                        aria-hidden="true"
                      />{:else if fact.branch}<GitBranch
                        size={11}
                        aria-hidden="true"
                      />{:else if fact.commit}<GitCommitHorizontal
                        size={11}
                        aria-hidden="true"
                      />{/if}{#if fact.text !== ""}<span class="fact-text">{fact.text}</span
                      >{/if}</span
                  >
                {/if}
                {#if node.kind.type === "dir" && node.kind.unlisted?.type === "large"}
                  <span class="tag" title={t("tree.largeFolderTitle", { n: 5000 })}>
                    {t("tree.largeFolder")}
                  </span>
                {:else if node.kind.type === "dir" && node.kind.unlisted?.type === "ignored"}
                  <span class="tag" title={t("explorer.ignoredHint")}>{t("explorer.ignored")}</span>
                {/if}
                {#each said as badge, n (n)}
                  <Badge {badge} />
                {/each}
              {/if}
            </div>
          {/if}
        {/each}
      </div>
    </div>
    {#if projects.strip.kind !== "none"}
      <ProjectStrip
        focusNode={(path) => {
          store.reveal(path);
          list?.focus();
        }}
      />
    {:else if store.codeOffer.status === "offered" && !projects.askedThisOpen}
      <div class="code-offer" role="region" aria-label={t("explorer.code.label")}>
        <span>{tn("explorer.code.offer", store.codeOffer.hidden)}</span>
        <button type="button" class="btn btn--sm" onclick={() => void store.acceptCodeOffer()}>
          {t("explorer.code.show")}
        </button>
        <button type="button" class="link" onclick={() => void store.declineCodeOffer()}>
          {t("explorer.code.decline")}
        </button>
      </div>
    {:else if note !== null && !projects.askedThisOpen}
      {#key note}
        {#await note.view() then loaded}
          <loaded.default focusList={() => list?.focus()} />
        {/await}
      {/key}
    {/if}
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
{#if projects.card !== null && cardRow !== null}
  <!-- the card's code loads the first time a card shows (a plain folder never needs it) -->
  {@const open = projects.card}
  {@const about = cardRow}
  {#await projectCard() then loaded}
    <loaded.default
      row={about}
      anchor={open?.anchor ?? new DOMRect()}
      from={open?.from ?? "hover"}
      close={closeCard}
      hovering={cardHovering}
      openTab={cardOpenTab}
      openSettings={cardSettings}
    />
  {/await}
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
    container: explorer / inline-size;
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
    box-shadow: inset 0 0 0 2px var(--accent);
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
  /* the folder a drop lands in: fill + ring (selection has no ring, keyboard focus a thin one); its contents read as
     one block with it. No transition: the mark follows the pointer. */
  .row.drop-target {
    background: var(--accent-drop);
    box-shadow: inset 0 0 0 2px var(--accent);
  }
  .row.drop-block {
    background: var(--accent-soft);
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
  .icon-cell.project {
    color: var(--hue);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name.named {
    display: flex;
    gap: var(--sp-2);
  }
  /* the folder name shrinks only after the short name is cut to nothing */
  .named .folder {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .named .alias {
    flex: 0 100000 auto;
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    text-overflow: ellipsis;
  }
  /* a project node with a fact: the name keeps its width, the fact gives way first */
  .name.with-fact {
    flex: 1 1 auto;
  }
  .fact {
    flex: 0 100000 auto;
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* a state: the status bar's chip tint, text in the text color (AA on every row tint); never cut, the name gives way */
  .fact.chip {
    flex: none;
    padding: 0 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--git-modified) 18%, transparent);
    color: var(--text);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    letter-spacing: 0.02em;
  }
  /* on the selection's tint, muted text is drawn a little darker (AA contrast) */
  .row.is-selected .fact:not(.chip) {
    color: color-mix(in srgb, var(--muted) 60%, var(--text));
  }
  .fact-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* git can't read the project: a warning mark, kept when the branch is cut; a branch: its mark, as the status bar's */
  .fact.warn,
  .fact.marked {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .fact.marked :global(svg) {
    flex: none;
  }
  .fact.warn :global(svg) {
    flex: none;
    color: var(--git-modified-text);
  }
  @container explorer (max-width: 239px) {
    .fact:not(.warn) {
      display: none;
    }
    .fact.warn .fact-text {
      display: none;
    }
  }
  .tag {
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
  .code-offer {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-2);
    padding: 6px var(--sp-3);
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
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
