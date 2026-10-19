<script lang="ts">
  // The open workspaces of the process as chips on the top bar, in list order: a colored badge by kind, the name (cut
  // in the middle when long), the window count, a close button. Click | Enter focuses the window that shows it; the
  // arrows move between chips; Delete, a middle click or the close button closes it; the context menu (right click,
  // Menu key, Shift+F10) opens it in a new window or closes it. Too many for the width: chips shrink, then the row
  // scrolls sideways with faded edges, keeping this window's chip in view.
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import X from "@lucide/svelte/icons/x";
  import type { WorkspaceSummary } from "../../api";
  import { keyHint } from "../commands/hint";
  import ContextMenu from "../explorer/ContextMenu.svelte";
  import type { MenuEntry } from "../explorer/menu";
  import { t, tn } from "../i18n";
  import { chipsTight, hueOf, splitName } from "./switcherModel";
  import { switcher } from "./workspaceList.svelte";

  type Props = { current: number | null };
  let { current }: Props = $props();

  /** What the top bar's own buttons and padding take next to the chips (px). */
  const BUTTONS = 96;

  let row: HTMLElement | undefined = $state();
  let menu = $state.raw<{ x: number; y: number; entries: readonly MenuEntry[] } | null>(null);
  /** Which edges hide chips (faded). */
  let hidden = $state<{ start: boolean; end: boolean }>({ start: false, end: false });
  /** Too many for the width: names keep their start only (a cut in the middle needs room). */
  let tight = $state(false);

  /** The chip the keyboard reaches with Tab: the one this window shows, else the first. */
  const reachable = $derived(
    switcher.list.some((w) => w.id === current) ? current : (switcher.list[0]?.id ?? null),
  );
  const closeKeys = $derived(keyHint("app.closeWorkspace"));

  function nameOf(workspace: WorkspaceSummary): string {
    return workspace.kind === "notes" ? t("notes.title") : workspace.title;
  }

  function closeLabel(workspace: WorkspaceSummary): string {
    const label = t("switcher.closeOne", { name: nameOf(workspace) });
    return workspace.id === current && closeKeys !== null ? `${label} (${closeKeys})` : label;
  }

  function entriesFor(workspace: WorkspaceSummary): MenuEntry[] {
    return [
      {
        kind: "item",
        label: t("switcher.openNew"),
        run: () => switcher.openInNewWindow(workspace),
      },
      {
        kind: "item",
        label: t("switcher.close"),
        hint: workspace.id === current ? (closeKeys ?? undefined) : undefined,
        run: () => switcher.close(workspace.id),
      },
    ];
  }

  function openMenu(workspace: WorkspaceSummary, x: number, y: number): void {
    menu = { x, y, entries: entriesFor(workspace) };
  }

  function oncontextmenu(e: MouseEvent, workspace: WorkspaceSummary): void {
    e.preventDefault();
    openMenu(workspace, e.clientX, e.clientY);
  }

  function onauxclick(e: MouseEvent, workspace: WorkspaceSummary): void {
    if (e.button !== 1) return;
    e.preventDefault();
    switcher.close(workspace.id);
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
    if (e.key === "Delete" && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      switcher.close(workspace.id);
      return;
    }
    const chips = Array.from(row?.querySelectorAll<HTMLElement>("[data-tab]") ?? []);
    const at = chips.indexOf(e.currentTarget as HTMLElement);
    const to =
      e.key === "ArrowRight"
        ? (at + 1) % chips.length
        : e.key === "ArrowLeft"
          ? (at - 1 + chips.length) % chips.length
          : e.key === "Home"
            ? 0
            : e.key === "End"
              ? chips.length - 1
              : -1;
    if (to < 0) return;
    e.preventDefault();
    chips[to]?.focus();
  }

  function measure(): void {
    if (!row) return;
    const { scrollLeft, scrollWidth, clientWidth } = row;
    const start = scrollLeft > 1;
    const end = scrollLeft + clientWidth < scrollWidth - 1;
    if (start !== hidden.start || end !== hidden.end) hidden = { start, end };
    // the room is the bar's (minus its buttons), not the row's: the row's own width follows the names
    const bar = row.closest("nav")?.parentElement?.clientWidth ?? 0;
    tight = chipsTight(bar - BUTTONS, switcher.list.length);
  }

  // the row's edges (scroll + size) and the wheel: a vertical wheel scrolls the row sideways
  $effect(() => {
    const el = row;
    if (!el) return;
    const onwheel = (e: WheelEvent): void => {
      if (el.scrollWidth <= el.clientWidth || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
      e.preventDefault();
      el.scrollLeft += e.deltaY;
    };
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    const bar = el.closest("nav")?.parentElement;
    if (bar) observer.observe(bar);
    el.addEventListener("scroll", measure, { passive: true });
    el.addEventListener("wheel", onwheel, { passive: false });
    return () => {
      observer.disconnect();
      el.removeEventListener("scroll", measure);
      el.removeEventListener("wheel", onwheel);
    };
  });

  // this window's chip stays in view when it changes or the list moves
  $effect(() => {
    const id = current;
    void switcher.list;
    const chip = row?.querySelector<HTMLElement>(`[data-id="${id}"]`);
    chip?.scrollIntoView({ block: "nearest", inline: "nearest" });
    measure();
  });
</script>

<nav class="switcher" aria-label={t("switcher.label")}>
  <ul
    bind:this={row}
    class="row"
    class:fade-start={hidden.start}
    class:fade-end={hidden.end}
    class:tight
  >
    {#each switcher.list as workspace (workspace.id)}
      {@const name = nameOf(workspace)}
      {@const cut = splitName(name, workspace.kind === "singleFile")}
      <li
        class="chip"
        class:is-current={workspace.id === current}
        data-id={workspace.id}
        style:--hue="var(--tag-{hueOf(workspace)})"
      >
        <button
          type="button"
          class="pick"
          data-tab
          aria-current={workspace.id === current ? "page" : undefined}
          aria-label={workspace.windows > 1
            ? `${name}, ${tn("switcher.windows", workspace.windows)}`
            : name}
          tabindex={workspace.id === reachable ? 0 : -1}
          title={workspace.path === "" || workspace.kind === "notes" ? name : workspace.path}
          onclick={() => switcher.focus(workspace.id)}
          oncontextmenu={(e) => oncontextmenu(e, workspace)}
          onauxclick={(e) => onauxclick(e, workspace)}
          onmousedown={(e) => {
            if (e.button === 1) e.preventDefault();
          }}
          onkeydown={(e) => onkeydown(e, workspace)}
        >
          <span class="badge" aria-hidden="true">
            {#if workspace.kind === "project"}
              <Folder size={13} strokeWidth={2.2} />
            {:else if workspace.kind === "notes"}
              <StickyNote size={13} strokeWidth={2.2} />
            {:else}
              <FileText size={13} strokeWidth={2.2} />
            {/if}
          </span>
          <span class="name" aria-hidden="true"
            ><span class="head">{cut.head}</span>{#if cut.tail !== ""}<span class="tail"
                >{cut.tail}</span
              >{/if}</span
          >
          {#if workspace.windows > 1}
            <span class="count" aria-hidden="true" title={tn("switcher.windows", workspace.windows)}
              >{workspace.windows}</span
            >
          {/if}
        </button>
        <button
          type="button"
          class="close"
          tabindex="-1"
          aria-label={closeLabel(workspace)}
          title={closeLabel(workspace)}
          onclick={() => switcher.close(workspace.id)}
        >
          <X size={13} strokeWidth={2.2} aria-hidden="true" />
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
  .switcher {
    display: flex;
    flex: 0 1 auto;
    min-width: 0;
    height: 100%;
  }
  .row {
    --fade: 28px;
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
    margin: 0;
    padding: 0 2px;
    overflow-x: auto;
    overflow-y: hidden;
    list-style: none;
    scrollbar-width: none;
  }
  .row::-webkit-scrollbar {
    display: none;
  }
  .row.fade-start {
    mask-image: linear-gradient(to right, transparent, var(--text) var(--fade));
  }
  .row.fade-end {
    mask-image: linear-gradient(to left, transparent, var(--text) var(--fade));
  }
  .row.fade-start.fade-end {
    mask-image: linear-gradient(
      to right,
      transparent,
      var(--text) var(--fade),
      var(--text) calc(100% - var(--fade)),
      transparent
    );
  }
  .chip {
    position: relative;
    display: flex;
    flex: 0 1 auto;
    align-items: center;
    min-width: 96px;
    max-width: 232px;
    height: 30px;
    border: 1px solid transparent;
    border-radius: 9px;
    color: var(--muted);
    transition:
      background-color var(--dur-fast) var(--ease),
      border-color var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease),
      box-shadow var(--dur-fast) var(--ease);
    animation: chip-in var(--dur) var(--ease);
  }
  .chip:hover {
    background: var(--hover);
    color: var(--text);
  }
  .chip.is-current {
    background: var(--bg);
    border-color: var(--border);
    box-shadow: var(--shadow-chip);
    color: var(--text);
  }
  .pick {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 7px;
    min-width: 0;
    height: 100%;
    padding: 0 2px 0 5px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: inherit;
    font-size: var(--fs-ui);
    cursor: pointer;
  }
  .pick:focus-visible {
    outline-offset: -2px;
  }
  .badge {
    display: grid;
    flex: none;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 6px;
    background: color-mix(in srgb, var(--hue) 15%, transparent);
    color: var(--hue);
  }
  .name {
    display: flex;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
  }
  .tight .tail {
    display: none;
  }
  /* tight: the other chips give the close button's room to the name; it shows over the name's end on hover */
  .tight .chip:not(.is-current) .pick {
    padding-right: var(--sp-2);
  }
  .tight .chip:not(.is-current) .close {
    position: absolute;
    right: 4px;
    margin: 0;
    background: color-mix(in srgb, var(--text) 6%, var(--surface));
  }
  .tight .chip:not(.is-current) .close:hover {
    background: color-mix(in srgb, var(--text) 12%, var(--surface));
  }
  .head {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tail {
    flex: none;
  }
  .is-current .name {
    font-weight: 600;
  }
  .count {
    flex: none;
    min-width: 16px;
    padding: 0 4px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    line-height: 14px;
    text-align: center;
  }
  .close {
    display: grid;
    flex: none;
    place-items: center;
    width: 20px;
    height: 20px;
    margin-right: 4px;
    padding: 0;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--faint);
    cursor: pointer;
    opacity: 0;
    transition:
      opacity var(--dur-fast) var(--ease),
      background-color var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }
  .chip:hover .close,
  .chip:focus-within .close,
  .chip.is-current .close {
    opacity: 1;
  }
  .close:hover {
    background: var(--pressed);
    color: var(--text);
  }
  @keyframes chip-in {
    from {
      opacity: 0;
      transform: translateY(2px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .chip,
    .close {
      animation: none;
      transition: none;
    }
  }
</style>
