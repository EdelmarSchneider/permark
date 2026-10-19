<script lang="ts">
  // The bar of open workspaces (shown with two or more): one tab per workspace, in the order the process lists them.
  // Click | Enter focuses the window that shows it; the arrows move between tabs; the context menu (right click, Menu
  // key, Shift+F10) opens it in a new window or closes it.
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import type { WorkspaceSummary } from "../../api";
  import ContextMenu from "../explorer/ContextMenu.svelte";
  import type { MenuEntry } from "../explorer/menu";
  import { t, tn } from "../i18n";
  import { switcher } from "./workspaceList.svelte";

  type Props = { current: number | null };
  let { current }: Props = $props();

  let bar: HTMLElement | undefined = $state();
  let menu = $state.raw<{ x: number; y: number; entries: readonly MenuEntry[] } | null>(null);

  /** The tab the keyboard reaches with Tab: the one this window shows, else the first. */
  const reachable = $derived(
    switcher.list.some((w) => w.id === current) ? current : (switcher.list[0]?.id ?? null),
  );

  function entriesFor(workspace: WorkspaceSummary): MenuEntry[] {
    return [
      {
        kind: "item",
        label: t("switcher.openNew"),
        run: () => switcher.openInNewWindow(workspace),
      },
      { kind: "item", label: t("switcher.close"), run: () => switcher.close(workspace.id) },
    ];
  }

  function openMenu(workspace: WorkspaceSummary, x: number, y: number): void {
    menu = { x, y, entries: entriesFor(workspace) };
  }

  function oncontextmenu(e: MouseEvent, workspace: WorkspaceSummary): void {
    e.preventDefault();
    openMenu(workspace, e.clientX, e.clientY);
  }

  function onkeydown(e: KeyboardEvent, workspace: WorkspaceSummary): void {
    if (e.isComposing) return;
    if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      const box =
        e.currentTarget instanceof Element ? e.currentTarget.getBoundingClientRect() : null;
      openMenu(workspace, box?.left ?? 0, box?.bottom ?? 0);
      return;
    }
    const tabs = Array.from(bar?.querySelectorAll<HTMLElement>("[data-tab]") ?? []);
    const at = tabs.indexOf(e.currentTarget as HTMLElement);
    const to =
      e.key === "ArrowRight"
        ? (at + 1) % tabs.length
        : e.key === "ArrowLeft"
          ? (at - 1 + tabs.length) % tabs.length
          : e.key === "Home"
            ? 0
            : e.key === "End"
              ? tabs.length - 1
              : -1;
    if (to < 0) return;
    e.preventDefault();
    tabs[to]?.focus();
  }
</script>

<nav class="bar" aria-label={t("switcher.label")} bind:this={bar}>
  <ul>
    {#each switcher.list as workspace (workspace.id)}
      <li>
        <button
          type="button"
          class="tab"
          data-tab
          aria-current={workspace.id === current ? "true" : undefined}
          tabindex={workspace.id === reachable ? 0 : -1}
          title={workspace.path === "" ? workspace.title : workspace.path}
          onclick={() => switcher.focus(workspace.id)}
          oncontextmenu={(e) => oncontextmenu(e, workspace)}
          onkeydown={(e) => onkeydown(e, workspace)}
        >
          {#if workspace.kind === "project"}
            <Folder size={14} aria-hidden="true" />
          {:else if workspace.kind === "notes"}
            <StickyNote size={14} aria-hidden="true" />
          {:else}
            <FileText size={14} aria-hidden="true" />
          {/if}
          <span class="title"
            >{workspace.kind === "notes" ? t("notes.title") : workspace.title}</span
          >
          {#if workspace.windows > 1}
            <span class="count" title={tn("switcher.windows", workspace.windows)}>
              {workspace.windows}
              <span class="sr-only">{tn("switcher.windows", workspace.windows)}</span>
            </span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>
</nav>
{#if menu}
  <ContextMenu
    x={menu.x}
    y={menu.y}
    entries={menu.entries}
    close={() => (menu = null)}
    label={t("switcher.menu")}
  />
{/if}

<style>
  .bar {
    height: 100%;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .bar::-webkit-scrollbar {
    display: none;
  }
  ul {
    display: flex;
    align-items: stretch;
    height: 100%;
    margin: 0;
    padding: 0 var(--sp-2);
    list-style: none;
  }
  li {
    display: flex;
    flex: none;
  }
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 240px;
    height: 100%;
    padding: 0 var(--sp-3);
    border: 0;
    border-left: 1px solid transparent;
    border-right: 1px solid transparent;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-sm);
    cursor: pointer;
    transition:
      background-color var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }
  .tab:hover {
    background: var(--hover);
    color: var(--text);
  }
  .tab[aria-current="true"] {
    background: var(--surface);
    border-left-color: var(--border);
    border-right-color: var(--border);
    box-shadow: inset 0 2px 0 var(--accent);
    color: var(--text);
    font-weight: 600;
  }
  .tab :global(svg) {
    flex: none;
  }
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    flex: none;
    min-width: 16px;
    padding: 0 5px;
    border-radius: 8px;
    background: var(--accent-soft);
    color: var(--accent-text);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    line-height: 16px;
    text-align: center;
  }
</style>
