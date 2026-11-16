<script module lang="ts">
  import type { Component } from "svelte";

  /** A tab: the view's id, its label, its icon (crowded strips show it alone), a short count. */
  export type TabView = {
    id: string;
    label: string;
    icon: Component<{ size?: number }> | undefined;
    badge: string | null;
  };
</script>

<script lang="ts">
  // The views of one group of a region as tabs (Files | Search | plugin tabs at the left, Outline | Live changes at the
  // right): drawn by the region when the group shows two views or more, or one view without a header of its own.
  // Selecting shows a view, never hides it. One Tab stop: arrows move between the tabs, Enter | Space select; right
  // click | Shift+F10 | the Menu key open the view's options. After the tabs, where the window has views to arrange,
  // one options button for the tab shown: a Tab stop of its own, seen while the strip is pointed at | has the focus.
  // A crowded strip too narrow for the whole label of the tab shown moves its last other tabs into a menu of its views.
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import { tick } from "svelte";
  import ContextMenu from "../explorer/ContextMenu.svelte";
  import type { MenuEntry } from "../explorer/menu";
  import { t } from "../i18n";
  import { rowFit } from "./rowFit";

  type Props = {
    views: readonly TabView[];
    selected: string | null;
    onselect: (id: string) => void;
    /** The view's options at a point (`null`: under the tab). */
    onmenu?: (id: string, at: { x: number; y: number }, anchor: HTMLElement) => void;
    /** The options button was pressed, for the tab shown (absent: the strip draws no button). */
    onoptions?: (id: string, button: HTMLElement) => void;
    /** The options of the tab shown are open. */
    expanded?: boolean;
    /** A pointer went down on a tab: the handle its view is dragged by (absent: the tabs are not handles). */
    ongrab?: (id: string, tab: HTMLElement, event: PointerEvent) => void;
    /** The view being dragged (its tab is dimmed). */
    dragging?: string | null;
    /** The strip's name (default: the right panel's). */
    label?: string;
    /** Tabs that keep their labels: more make the strip crowded. */
    roomy?: number;
  };
  let {
    views,
    selected,
    onselect,
    onmenu,
    onoptions,
    expanded = false,
    ongrab,
    dragging = null,
    label,
    roomy = 2,
  }: Props = $props();

  /** Many tabs: the ones that are not open show their icon alone (the label is the tooltip). */
  const crowded = $derived(views.length > roomy);
  /** The tab the options button acts on: the one shown (nothing selected: the first). */
  const shown = $derived(views.find((v) => v.id === selected) ?? views[0] ?? null);

  /** The strip's own sizes (CSS below): its side padding, the gap between tabs, a tab drawn as its icon alone, the
   *  views button, the options button with its margin. */
  const STRIP_PAD = 16;
  const TAB_GAP = 2;
  const ICON_TAB = 24;
  const VIEWS_PX = 24;
  const OPTIONS_PX = 32;
  let bar: HTMLElement | undefined = $state();
  /** Px the tabs have, and the width of the selected tab with its whole label (0: not measured: every tab drawn). */
  let room = $state(0);
  let whole = $state(0);
  /** Crowded: the other tabs that stay beside the selected one's whole label; the last ones go into the views menu. */
  const others = $derived(views.filter((v) => v.id !== selected));
  const kept = $derived(
    crowded && selected !== null
      ? rowFit({
          room,
          fixed: whole,
          count: others.length,
          step: ICON_TAB + TAB_GAP,
          more: VIEWS_PX,
        })
      : others.length,
  );
  const folded = $derived(new Set(others.slice(kept).map((v) => v.id)));
  const drawn = $derived(views.filter((v) => !folded.has(v.id)));
  let list = $state.raw<{ x: number; y: number; anchor: HTMLElement } | null>(null);

  // the strip's width and the selected tab's whole width (its label, the language, a count change it) decide which
  // tabs it draws (observer torn down with the strip; none = every tab, as in tests). A resize is measured in the next
  // frame: tabs drawn again inside the observer's call would resize what it watches there ("ResizeObserver loop").
  $effect(() => {
    const id = selected;
    void shown?.label;
    void shown?.badge;
    if (!bar || typeof ResizeObserver === "undefined") return;
    const box = bar;
    const options = onoptions === undefined ? 0 : OPTIONS_PX;
    const measure = (): void => {
      room = box.clientWidth - STRIP_PAD - options;
      const tab = tabOf(id);
      const label = tab?.querySelector<HTMLElement>(".label");
      // the label is the only part of a tab that shortens: the rest of the tab plus the label's whole text (+1: the
      // measured text width is rounded, the drawn one is not)
      whole =
        tab && label
          ? Math.ceil(
              tab.getBoundingClientRect().width -
                label.getBoundingClientRect().width +
                label.scrollWidth +
                1,
            )
          : 0;
    };
    measure();
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(measure);
    });
    observer.observe(box);
    const tab = tabOf(id);
    if (tab) observer.observe(tab);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  });

  /** The views menu: every view of the strip, the one shown checked; picking one shows it and its tab takes the
   *  keyboard. */
  function viewEntries(): MenuEntry[] {
    return views.map((view) => ({
      kind: "item",
      label: `${view.label}${view.badge === null ? "" : ` (${view.badge})`}`,
      checked: view.id === selected,
      run: () => {
        if (view.id !== selected) onselect(view.id);
        void tick().then(() => tabOf(view.id)?.focus());
      },
    }));
  }

  function viewsMenu(button: HTMLElement): void {
    if (list !== null) {
      list = null;
      return;
    }
    const box = button.getBoundingClientRect();
    list = { x: box.left, y: box.bottom + 2, anchor: button };
  }

  /** The tab that takes the Tab stop (the selected one until the focus goes to another: arrows, a click, a script). */
  let focused = $state<string | null>(null);
  const stop = $derived(
    drawn.find((v) => v.id === focused)?.id ??
      drawn.find((v) => v.id === selected)?.id ??
      drawn[0]?.id ??
      null,
  );
  let strip: HTMLElement | undefined = $state();

  function tabOf(id: string | null): HTMLElement | null {
    return strip?.querySelector<HTMLElement>(`[data-view="${id ?? ""}"]`) ?? null;
  }

  function menuAt(id: string, el: HTMLElement): void {
    const box = el.getBoundingClientRect();
    onmenu?.(id, { x: box.left, y: box.bottom }, el);
  }

  function keydown(event: KeyboardEvent): void {
    if ((event.key === "F10" && event.shiftKey) || event.key === "ContextMenu") {
      const el = tabOf(stop);
      if (el !== null && stop !== null) {
        event.preventDefault();
        menuAt(stop, el);
      }
      return;
    }
    const at = drawn.findIndex((v) => v.id === stop);
    const last = drawn.length - 1;
    let to: number;
    switch (event.key) {
      case "ArrowRight":
        to = at >= last ? 0 : at + 1;
        break;
      case "ArrowLeft":
        to = at <= 0 ? last : at - 1;
        break;
      case "Home":
        to = 0;
        break;
      case "End":
        to = last;
        break;
      default:
        return;
    }
    event.preventDefault();
    const id = drawn[to]?.id ?? null;
    focused = id;
    tabOf(id)?.focus();
  }
</script>

<div class="bar" bind:this={bar}>
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div
    class="tabs"
    class:crowded
    role="tablist"
    aria-label={label ?? t("panel.tabs")}
    bind:this={strip}
    onkeydown={keydown}
    onfocusin={(event) => {
      const tab = event.target instanceof Element ? event.target.closest("[data-view]") : null;
      focused = tab?.getAttribute("data-view") ?? focused;
    }}
    onfocusout={(event) => {
      if (!(event.relatedTarget instanceof Node) || !strip?.contains(event.relatedTarget)) {
        focused = null;
      }
    }}
  >
    {#each drawn as view (view.id)}
      {@const iconOnly = view.icon !== undefined && crowded && selected !== view.id}
      <button
        type="button"
        role="tab"
        class="tab"
        class:icon-only={iconOnly}
        class:has-icon={view.icon !== undefined}
        class:has-badge={view.badge !== null}
        class:dragged={dragging === view.id}
        data-view={view.id}
        tabindex={stop === view.id ? 0 : -1}
        aria-selected={selected === view.id}
        aria-label={iconOnly
          ? view.badge === null
            ? view.label
            : `${view.label} ${view.badge}`
          : undefined}
        title={view.label}
        onclick={() => {
          if (view.id !== selected) onselect(view.id);
        }}
        onpointerdown={(event) => ongrab?.(view.id, event.currentTarget, event)}
        oncontextmenu={(event) => {
          if (onmenu === undefined || !(event.currentTarget instanceof HTMLElement)) return;
          event.preventDefault();
          onmenu(view.id, { x: event.clientX, y: event.clientY }, event.currentTarget);
        }}
      >
        {#if view.icon}
          {@const Icon = view.icon}
          <Icon size={14} />
        {/if}
        {#if !iconOnly}<span class="label">{view.label}</span>{/if}
        {#if view.badge !== null}{" "}<span class="badge">{view.badge}</span>{/if}
      </button>
    {/each}
  </div>
  {#if folded.size > 0}
    {@const name = label ?? t("panel.tabs")}
    <button
      type="button"
      class="icon-btn views"
      aria-label={name}
      title={name}
      aria-haspopup="menu"
      aria-expanded={list !== null}
      onclick={(event) => viewsMenu(event.currentTarget)}
    >
      <ChevronDown size={14} aria-hidden="true" />
    </button>
  {/if}
  {#if onoptions && shown}
    {@const name = t("panel.viewMenu", { name: shown.label })}
    <button
      type="button"
      class="icon-btn options"
      data-view-menu={shown.id}
      aria-label={name}
      title={name}
      aria-haspopup="menu"
      aria-expanded={expanded}
      onclick={(event) => onoptions(shown.id, event.currentTarget)}
    >
      <Ellipsis size={14} aria-hidden="true" />
    </button>
  {/if}
</div>
{#if list !== null}
  <ContextMenu
    x={list.x}
    y={list.y}
    entries={viewEntries()}
    label={label ?? t("panel.tabs")}
    anchor={list.anchor}
    close={() => (list = null)}
  />
{/if}

<style>
  /* the tabs, then the options button: one line over the views of the group */
  .bar {
    display: flex;
    flex: none;
    min-width: 0;
    border-bottom: 1px solid var(--border);
  }
  .tabs {
    display: flex;
    flex: 1 1 auto;
    min-width: 0;
    gap: 2px;
    padding: var(--sp-2) var(--sp-2) 0;
    overflow: hidden;
  }
  /* the views menu of a crowded strip, right after its tabs: always seen (it stands for the tabs it holds) */
  .views {
    flex: none;
    align-self: center;
    width: 24px;
    height: 24px;
  }
  .views[aria-expanded="true"] {
    background: var(--hover);
    color: var(--text);
  }
  /* a 24 px target at the end of the strip. Not seen until the strip is pointed at or holds the focus: by opacity
     alone, so it keeps its room and its Tab stop */
  .options {
    flex: none;
    align-self: center;
    width: 24px;
    height: 24px;
    margin-inline-end: var(--sp-2);
    opacity: 0;
  }
  .bar:hover .options,
  .bar:focus-within .options,
  .options[aria-expanded="true"] {
    opacity: 1;
  }
  .options[aria-expanded="true"] {
    background: var(--hover);
    color: var(--text);
  }
  /* nothing hovers on a touch screen: always there */
  @media (hover: none) {
    .options {
      opacity: 1;
    }
  }
  /* A strip too narrow for its labels takes the room from the tabs by their weights: the tab shown loses nothing
     while another can still give way (its weight is nothing beside theirs), so its label stays whole; with every
     other tab at its smallest (a crowded strip first moves its last other tabs into its views menu), it shortens
     too and every tab stays reachable. A tab is also the handle its view is dragged by: a pen | a finger on it never
     scrolls the page away from under a drag. */
  .tab {
    --tab-pad: var(--sp-3);
    flex: 0 100000 auto;
    min-width: 0;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 5px var(--tab-pad);
    border: 0;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-sm);
    cursor: pointer;
    touch-action: none;
  }
  .tab[aria-selected="true"] {
    flex-shrink: 1;
  }
  .tab.icon-only {
    --tab-pad: var(--sp-2);
    flex: none;
  }
  .crowded .tab {
    --tab-pad: var(--sp-2);
  }
  .crowded .tab.icon-only {
    --tab-pad: 5px;
  }
  .tab.dragged {
    opacity: 0.45;
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* A tab that gives way, with an icon: its label shortens (ellipsis) down to a few letters, then goes and the icon
     stays alone, in the middle of the room the tab still has (the tab's name is its tooltip). The room the label
     has in the tab, less that floor, times a large number: no limit above the floor, no width under it. */
  .tab.has-icon:not([aria-selected="true"]) {
    --label-floor: calc(14px + 6px + 3em);
    min-width: calc(14px + 2 * var(--tab-pad));
    justify-content: center;
  }
  .tab.has-icon.has-badge:not([aria-selected="true"]) {
    --label-floor: calc(14px + 6px + 3em + 6px + 2em);
  }
  .tab.has-icon:not([aria-selected="true"]) .label {
    max-width: max(0px, calc((100% - var(--label-floor)) * 999));
  }
  /* without an icon its letters are all it has: a few of them stay */
  .tab:not(.has-icon):not([aria-selected="true"]) .label {
    min-width: 3em;
  }
  .tab:hover {
    color: var(--text);
  }
  .tab[aria-selected="true"] {
    border-bottom-color: var(--accent);
    color: var(--text);
    font-weight: 600;
  }
  .badge {
    flex: none;
    min-width: 16px;
    padding: 0 5px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-fg);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    line-height: 16px;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }
</style>
