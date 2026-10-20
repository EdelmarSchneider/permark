<script lang="ts">
  // This window's tabs as chips on the top bar, in strip order: a colored badge by kind, the name (cut in the middle
  // when long), the unsaved dot, a close button. Click | Enter | Space shows the tab; the arrows move between chips;
  // Delete, a middle click or the close button closes it; the context menu (right click, Menu key, Shift+F10) moves it
  // to a new window or closes it. Too many for the width: chips shrink, then the row scrolls sideways with faded edges,
  // keeping the active chip in view.
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import X from "@lucide/svelte/icons/x";
  import type { TabSummary } from "../../api";
  import { keyHint } from "../commands/hint";
  import ContextMenu from "../explorer/ContextMenu.svelte";
  import type { MenuEntry } from "../explorer/menu";
  import { t } from "../i18n";
  import { windowStore } from "../window.svelte";
  import { chipsTight, hueOf, splitName } from "./switcherModel";
  import { tabs } from "./tabs.svelte";

  /** The active tab of this window. */
  const current = $derived(tabs.active);

  /** What the top bar's own buttons and padding take next to the chips (px). */
  const BUTTONS = 96;

  let row: HTMLElement | undefined = $state();
  let menu = $state.raw<{ x: number; y: number; entries: readonly MenuEntry[] } | null>(null);
  /** Which edges hide chips (faded). */
  let hidden = $state<{ start: boolean; end: boolean }>({ start: false, end: false });
  /** Too many for the width: names keep their start only (a cut in the middle needs room). */
  let tight = $state(false);

  /** The chip the keyboard reaches with Tab: the active one, else the first. */
  const reachable = $derived(
    tabs.list.some((tab) => tab.tab === current) ? current : (tabs.list[0]?.tab ?? null),
  );
  const closeKeys = $derived(keyHint("tab.close"));

  function nameOf(tab: TabSummary): string {
    return tab.kind === "notes" ? t("notes.title") : tab.title;
  }

  function closeLabel(tab: TabSummary): string {
    const label = t("switcher.closeOne", { name: nameOf(tab) });
    return tab.tab === current && closeKeys !== null ? `${label} (${closeKeys})` : label;
  }

  function entriesFor(tab: TabSummary): MenuEntry[] {
    const entries: MenuEntry[] = [];
    // moving the only tab out would leave an empty window behind
    if (tabs.list.length > 1) {
      entries.push(
        {
          kind: "item",
          label: t("switcher.moveNew"),
          run: () => {
            void tabs.moveToNewWindow(tab.tab).then((result) => {
              if (result === "refused") windowStore.notifyNote(t("switcher.moveFailed"));
            });
          },
        },
        { kind: "separator" },
      );
    }
    entries.push({
      kind: "item",
      label: t("switcher.close"),
      hint: tab.tab === current ? (closeKeys ?? undefined) : undefined,
      run: () => tabs.close(tab.tab),
    });
    return entries;
  }

  function openMenu(tab: TabSummary, x: number, y: number): void {
    menu = { x, y, entries: entriesFor(tab) };
  }

  function oncontextmenu(e: MouseEvent, tab: TabSummary): void {
    e.preventDefault();
    openMenu(tab, e.clientX, e.clientY);
  }

  function onauxclick(e: MouseEvent, tab: TabSummary): void {
    if (e.button !== 1) return;
    e.preventDefault();
    tabs.close(tab.tab);
  }

  function onkeydown(e: KeyboardEvent, tab: TabSummary): void {
    if (e.isComposing) return;
    if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      const box =
        e.currentTarget instanceof Element ? e.currentTarget.getBoundingClientRect() : null;
      openMenu(tab, box?.left ?? 0, box?.bottom ?? 0);
      return;
    }
    if (e.key === "Delete" && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      tabs.close(tab.tab);
      return;
    }
    // plain arrows | Home | End only: with modifiers they are the window's (Ctrl+Alt+arrows switch the tab)
    if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
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
    tight = chipsTight(bar - BUTTONS, tabs.list.length);
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

  // the active tab changed (keys, a click, an open): a chip holding the focus lets go of it, so no focus ring is left
  // on it (this view hidden now, or shown again later with the focus brought back) and the keys reach the page
  $effect(() => {
    void current;
    const focused = document.activeElement;
    if (focused instanceof HTMLElement && row?.contains(focused)) focused.blur();
  });

  // the active chip stays in view when it changes or the list moves
  $effect(() => {
    const id = current;
    void tabs.list;
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
    {#each tabs.list as tab (tab.tab)}
      {@const name = nameOf(tab)}
      {@const cut = splitName(name, tab.kind === "singleFile")}
      <li
        class="chip"
        class:is-current={tab.tab === current}
        data-id={tab.tab}
        style:--hue="var(--tag-{hueOf(tab)})"
      >
        <button
          type="button"
          class="pick"
          data-tab
          aria-current={tab.tab === current ? "page" : undefined}
          aria-label={tab.unsaved ? `${name}, ${t("switcher.unsaved")}` : name}
          tabindex={tab.tab === reachable ? 0 : -1}
          title={tab.path === "" || tab.kind === "notes" ? name : tab.path}
          onclick={() => tabs.activate(tab.tab)}
          oncontextmenu={(e) => oncontextmenu(e, tab)}
          onauxclick={(e) => onauxclick(e, tab)}
          onmousedown={(e) => {
            // a press never takes the focus from the page (the editor keeps it; no chip left focused)
            e.preventDefault();
          }}
          onkeydown={(e) => onkeydown(e, tab)}
        >
          <span class="badge" aria-hidden="true">
            {#if tab.kind === "project"}
              <Folder size={13} strokeWidth={2.2} />
            {:else if tab.kind === "notes"}
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
          {#if tab.unsaved}
            <span class="dot" data-unsaved title={t("switcher.unsaved")}></span>
          {/if}
        </button>
        <button
          type="button"
          class="close"
          tabindex="-1"
          aria-label={closeLabel(tab)}
          title={closeLabel(tab)}
          onclick={() => tabs.close(tab.tab)}
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
  /* the chips stand on the band's bottom edge: the active one runs down into the document area (one piece), the
     others float a little above it; every chip keeps the same box, so nothing moves when the active one changes */
  .row {
    --fade: 28px;
    display: flex;
    align-self: stretch;
    align-items: flex-end;
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
    isolation: isolate;
    display: flex;
    flex: 0 1 auto;
    align-items: center;
    min-width: 96px;
    max-width: 232px;
    height: 34px;
    padding-bottom: 4px;
    color: var(--muted);
    transition: color var(--dur-fast) var(--ease);
    animation: chip-in var(--dur) var(--ease);
  }
  /* what the chip looks like, behind its content: a quiet rounded chip, or (active) the tab joined to the page */
  .chip::before {
    position: absolute;
    inset: 0 0 4px;
    z-index: -1;
    border: 1px solid transparent;
    border-radius: 9px;
    content: "";
    transition:
      background-color var(--dur-fast) var(--ease),
      border-color var(--dur-fast) var(--ease);
  }
  .chip:hover {
    color: var(--text);
  }
  .chip:hover::before {
    background: var(--hover);
  }
  .chip.is-current {
    color: var(--text);
  }
  /* down to the band's bottom edge, over its border line: the page's color, no bottom edge, no seam */
  .chip.is-current::before {
    bottom: 0;
    border-color: var(--border);
    border-bottom-width: 0;
    border-radius: 9px 9px 0 0;
    background: var(--bg);
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
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
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
    .chip::before,
    .close {
      animation: none;
      transition: none;
    }
  }
</style>
