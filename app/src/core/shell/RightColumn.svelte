<script lang="ts">
  // The right column: its present views in groups one above the other (a group with several views = tabs), a divider
  // between groups, an options menu per view (move, show as tab, close, reset). Every present view is drawn ONCE in
  // one grid, placed by row: moving it to another group never remounts it (scroll, filters kept). Menus, dividers and
  // strips appear only with two views or more. Spec: shell.md §Frame (Right region).
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import ListTree from "@lucide/svelte/icons/list-tree";
  import { tick, type Component } from "svelte";
  import type { OutlineItem } from "../../api";
  import OutlinePanel from "../doc/OutlinePanel.svelte";
  import ContextMenu from "../explorer/ContextMenu.svelte";
  import type { MenuEntry } from "../explorer/menu";
  import { t } from "../i18n";
  import type { PanelViewProps } from "../plugins/props";
  import RightTabs, { type TabView } from "./RightTabs.svelte";
  import {
    canJoin,
    canMove,
    canSplit,
    defaultRightViews,
    joinAsTab,
    MIN_GROUP_PX,
    moveView,
    OUTLINE,
    resizePair,
    splitOut,
    type DrawnGroup,
  } from "./rightViews";
  import { rightViews } from "./rightViews.svelte";
  import { closeView, selectView } from "./sides";
  import ViewDivider from "./ViewDivider.svelte";

  type Props = { outline: readonly OutlineItem[] | null };
  let { outline }: Props = $props();

  /** A tab strip's height (px), for the "too short" check. */
  const STRIP_PX = 32;
  /** The divider's track: its own room (the line in its middle), so it never covers a row of a view. */
  const DIVIDER_PX = 7;

  let height = $state(0);
  const groups = $derived(rightViews.drawn);
  const present = $derived(groups.flatMap((g) => g.views));
  /** Arranging controls (options, dividers, strips) only with two views or more. */
  const arranging = $derived(present.length >= 2);

  // too short for the groups one above the other: drawn as one tab group (the arrangement stays as saved)
  $effect(() => {
    const stacked = rightViews.stacked;
    const strips = stacked.filter((g) => g.views.length > 1).length;
    const need =
      stacked.length * MIN_GROUP_PX + (stacked.length - 1) * DIVIDER_PX + strips * STRIP_PX;
    rightViews.short = stacked.length > 1 && height > 0 && height < need;
  });

  type Placed = {
    group: DrawnGroup;
    strip: number | null;
    content: number;
    divider: number | null;
  };

  /** Grid rows: per group a strip (tabs only), its content (its share), a divider before the next group. */
  const plan = $derived.by(() => {
    const tracks: string[] = [];
    const placed: Placed[] = [];
    groups.forEach((group, i) => {
      let strip: number | null = null;
      if (group.views.length > 1) {
        tracks.push("auto");
        strip = tracks.length;
      }
      tracks.push(`minmax(0, ${Math.max(0.001, group.share * 100).toFixed(3)}fr)`);
      const content = tracks.length;
      let divider: number | null = null;
      if (i < groups.length - 1) {
        tracks.push(`${DIVIDER_PX}px`);
        divider = tracks.length;
      }
      placed.push({ group, strip, content, divider });
    });
    return { tracks: tracks.join(" "), placed };
  });

  type Item =
    | { key: string; kind: "strip"; place: Placed }
    | { key: string; kind: "view"; id: string; place: Placed }
    | { key: string; kind: "line"; place: Placed; next: Placed };

  /**
   * What the grid holds, in the order it is seen (= the Tab order): a group's strip, its views, the divider below it.
   * One keyed list: a view moved to another group keeps its element (never remounted).
   */
  const items = $derived(
    plan.placed.flatMap((place, i): Item[] => {
      const next = plan.placed[i + 1];
      return [
        ...(place.strip !== null ? [{ key: `strip:${i}`, kind: "strip" as const, place }] : []),
        ...place.group.views.map((id) => ({ key: `view:${id}`, kind: "view" as const, id, place })),
        ...(place.divider !== null && next !== undefined
          ? [{ key: `line:${i}`, kind: "line" as const, place, next }]
          : []),
      ];
    }),
  );

  function labelOf(id: string): string {
    if (id === OUTLINE) return t("outline.tab");
    const panel = rightViews.panels.find((p) => p.id === id);
    return panel === undefined ? id : t(panel.label);
  }

  function tabsOf(group: DrawnGroup): TabView[] {
    return group.views.map((id) => {
      const panel = rightViews.panels.find((p) => p.id === id);
      return {
        id,
        label: labelOf(id),
        icon: id === OUTLINE ? ListTree : panel?.icon,
        badge: panel?.badge?.() ?? null,
      };
    });
  }

  /** The view a group shows: alone, or its selected tab. */
  function shownIn(group: DrawnGroup): string | null {
    return rightViews.selectedOf(group);
  }

  function heightOf(group: DrawnGroup): number {
    const id = shownIn(group);
    const el = document.querySelector<HTMLElement>(`[data-right-view="${id ?? ""}"]`);
    return el?.getBoundingClientRect().height ?? 0;
  }

  // ---- the options menu of a view
  let menu = $state<{ id: string; x: number; y: number; anchor: HTMLElement } | null>(null);

  function openMenu(id: string, at: { x: number; y: number }, anchor: HTMLElement): void {
    menu = { id, x: at.x, y: at.y, anchor };
  }

  function openFrom(id: string, button: HTMLElement): void {
    if (menu?.id === id) {
      menu = null;
      return;
    }
    const box = button.getBoundingClientRect();
    openMenu(id, { x: box.left, y: box.bottom + 2 }, button);
  }

  /** After a move: the moved view's options (Show as tab: its tab) take the keyboard; the move is said. */
  async function moved(id: string, what: "up" | "down" | "tab" | "above" | "below"): Promise<void> {
    const name = labelOf(id);
    const words = {
      up: t("panel.moved.up", { name }),
      down: t("panel.moved.down", { name }),
      tab: t("panel.moved.tab", { name }),
      above: t("panel.moved.above", { name }),
      below: t("panel.moved.below", { name }),
    } as const;
    rightViews.status = words[what];
    await tick();
    const target =
      what === "tab"
        ? document.querySelector<HTMLElement>(`[role="tab"][data-view="${id}"]`)
        : document.querySelector<HTMLElement>(`[data-view-menu="${id}"]`);
    target?.focus();
  }

  function entriesFor(id: string): MenuEntry[] {
    const layout = rightViews.layout;
    const stacked = rightViews.stacked;
    const drawnAt = stacked.map((g) => g.at);
    const short = rightViews.short;
    const tabbed = (stacked.find((g) => g.views.includes(id))?.views.length ?? 0) > 1;
    const arrange = (next: typeof layout, what: "up" | "down" | "tab" | "above" | "below") => {
      rightViews.arrange(next);
      if (what === "tab") selectView(id);
      void moved(id, what);
    };
    const entries: MenuEntry[] = [];
    if (short) entries.push({ kind: "heading", label: t("panel.tooShort") });
    if (tabbed) {
      entries.push(
        {
          kind: "item",
          label: t("panel.showAbove"),
          disabled: short || !canSplit(layout, id),
          run: () => arrange(splitOut(layout, id, "above"), "above"),
        },
        {
          kind: "item",
          label: t("panel.showBelow"),
          disabled: short || !canSplit(layout, id),
          run: () => arrange(splitOut(layout, id, "below"), "below"),
        },
      );
    } else {
      entries.push(
        {
          kind: "item",
          label: t("panel.moveUp"),
          disabled: short || !canMove(layout, id, -1, drawnAt),
          run: () => arrange(moveView(layout, id, -1, drawnAt), "up"),
        },
        {
          kind: "item",
          label: t("panel.moveDown"),
          disabled: short || !canMove(layout, id, 1, drawnAt),
          run: () => arrange(moveView(layout, id, 1, drawnAt), "down"),
        },
        {
          kind: "item",
          label: t("panel.showAsTab"),
          disabled: short || !canJoin(layout, id, drawnAt),
          run: () => arrange(joinAsTab(layout, id, drawnAt), "tab"),
        },
      );
    }
    entries.push(
      { kind: "separator" },
      { kind: "item", label: t("panel.closeView"), run: () => closeView(id) },
      { kind: "separator" },
      {
        kind: "item",
        label: t("panel.resetLayout"),
        run: () => {
          rightViews.arrange(defaultRightViews());
          rightViews.status = t("panel.layoutReset");
        },
      },
    );
    return entries;
  }

  /** Right click | Shift+F10 | the Menu key on a view's header: its options. */
  function headerOf(event: Event): HTMLElement | null {
    const target = event.target;
    if (!arranging || !(target instanceof Element)) return null;
    return target.closest<HTMLElement>("[data-view-header]");
  }

  function headerMenu(id: string, event: MouseEvent): void {
    const header = headerOf(event);
    if (header === null) return;
    event.preventDefault();
    openMenu(id, { x: event.clientX, y: event.clientY }, header);
  }

  function headerKeys(id: string, event: KeyboardEvent): void {
    if (!((event.key === "F10" && event.shiftKey) || event.key === "ContextMenu")) return;
    const header = headerOf(event);
    if (header === null || !(event.target instanceof HTMLElement)) return;
    event.preventDefault();
    const box = event.target.getBoundingClientRect();
    openMenu(id, { x: box.left, y: box.bottom + 2 }, event.target);
  }
</script>

<div class="column" bind:clientHeight={height} style:grid-template-rows={plan.tracks}>
  {#each items as item (item.key)}
    {#if item.kind === "strip"}
      <div class="strip" style:grid-row={item.place.strip}>
        <RightTabs
          views={tabsOf(item.place.group)}
          selected={shownIn(item.place.group)}
          onselect={selectView}
          onmenu={openMenu}
        />
      </div>
    {:else if item.kind === "line"}
      {@const top = shownIn(item.place.group) ?? ""}
      {@const bottom = shownIn(item.next.group) ?? ""}
      <div class="line" style:grid-row={item.place.divider}>
        <ViewDivider
          top={labelOf(top)}
          bottom={labelOf(bottom)}
          controls={`right-view-${top}`}
          measure={() => ({ above: heightOf(item.place.group), below: heightOf(item.next.group) })}
          onsplit={(ratio) =>
            rightViews.arrange(
              resizePair(rightViews.layout, item.place.group.at, item.next.group.at, ratio),
            )}
        />
      </div>
    {:else}
      {@const id = item.id}
      {@const tabbed = item.place.group.views.length > 1}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="view"
        class:tabbed
        id={`right-view-${id}`}
        data-right-view={id}
        style:grid-row={item.place.content}
        hidden={tabbed && shownIn(item.place.group) !== id}
        oncontextmenu={(event) => headerMenu(id, event)}
        onkeydown={(event) => headerKeys(id, event)}
      >
        {#snippet options()}
          <button
            type="button"
            class="options"
            data-view-menu={id}
            aria-label={t("panel.viewMenu", { name: labelOf(id) })}
            title={t("panel.viewMenu", { name: labelOf(id) })}
            aria-haspopup="menu"
            aria-expanded={menu?.id === id}
            onclick={(event) => openFrom(id, event.currentTarget)}
          >
            <Ellipsis size={14} aria-hidden="true" />
          </button>
        {/snippet}
        {#if id === OUTLINE}
          {#if outline}<OutlinePanel
              items={outline}
              viewMenu={arranging ? options : undefined}
            />{/if}
        {:else}
          {@const panel = rightViews.panels.find((p) => p.id === id)}
          {#if panel}
            {#await panel.view() then loaded}
              {@const PanelView = loaded.default as unknown as Component<PanelViewProps>}
              <PanelView viewMenu={arranging ? options : undefined} />
            {/await}
          {/if}
        {/if}
      </div>
    {/if}
  {/each}
  <p class="sr-only" role="status">{rightViews.status}</p>
</div>

{#if menu}
  {@const open = menu}
  <ContextMenu
    x={open.x}
    y={open.y}
    entries={entriesFor(open.id)}
    label={t("panel.viewMenu", { name: labelOf(open.id) })}
    anchor={open.anchor}
    close={() => (menu = null)}
  />
{/if}

<style>
  .column {
    position: relative;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .strip,
  .line {
    grid-column: 1;
    min-width: 0;
  }
  .view {
    grid-column: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .view[hidden] {
    display: none;
  }
  /* a view shown as a tab: its tab names it, the header's title stays for screen readers only */
  .view.tabbed :global([data-view-header] h2) {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
  .options {
    display: inline-grid;
    flex: none;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .options:hover,
  .options[aria-expanded="true"] {
    background: var(--hover);
    color: var(--text);
  }
</style>
