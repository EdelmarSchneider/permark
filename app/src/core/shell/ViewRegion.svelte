<script lang="ts">
  // One region of the window (left | right | bottom): its present views in groups along its stack (one above the
  // other at the sides, side by side at the bottom), a group with several views = tabs, a divider between groups. It
  // draws the strips, the dividers and one SLOT per present view, in the order they are seen; the views themselves
  // are created once for the window (`ViewHosts`) and moved into their slots, so a view that changes group | region
  // is never created again (what it holds stays).
  import { onDestroy } from "svelte";
  import { t } from "../i18n";
  import ViewTabs, { type TabView } from "./ViewTabs.svelte";
  import {
    axisOf,
    DIVIDER_PX,
    resizePair,
    STRIP_PX,
    withRegion,
    type Axis,
    type DrawnGroup,
    type Region,
  } from "./arrangement";
  import { arrangement } from "./arrangement.svelte";
  import { selectView } from "./sides";
  import ViewDivider from "./ViewDivider.svelte";
  import { viewMenu } from "./viewMenu.svelte";
  import { viewSlots } from "./viewSlots.svelte";

  type Props = { region: Region };
  let { region }: Props = $props();

  const groups = $derived(arrangement.drawn(region));
  /** The way the groups follow each other. */
  const axis = $derived(axisOf(region));
  /** Arranging controls (the strips' menus, the headers' menus) only with two views or more. */
  const arranging = $derived(viewMenu.arranging(region));

  /** The name of the region's tab strips, and how many tabs keep their labels before a strip is crowded. */
  const strips = $derived.by(() => {
    switch (region) {
      case "left":
        return { label: t("sidebar.views"), roomy: 3 };
      case "right":
        return { label: t("panel.tabs"), roomy: 2 };
      case "bottom":
        return { label: t("panel.bottomTabs"), roomy: 5 };
      default:
        return region satisfies never;
    }
  });

  // the region's extent along its stack (the height at the sides, the width at the bottom) decides whether its
  // groups fit there (else one tab group): the store works that out from the size reported here; a region that is
  // gone has no size
  function measured(along: Axis, px: number | null | undefined): void {
    if (along === axis) arrangement.setSize(region, px ?? 0);
  }
  onDestroy(() => arrangement.setSize(region, 0));

  type Placed = {
    group: DrawnGroup;
    strip: number | null;
    content: number;
    divider: number | null;
  };

  /**
   * Grid tracks along the stack: per group its content (its share), a divider before the next group. Groups one above
   * the other: a strip (when the group draws one) takes a track of its own before the content. Side by side: it sits
   * in the content's track, in a row over it. A strip's own height is `STRIP_PX`, the room the fold rule counts.
   */
  const plan = $derived.by(() => {
    const tracks: string[] = [];
    const placed: Placed[] = [];
    groups.forEach((group, i) => {
      const tabs = arrangement.hasStrip(group);
      if (tabs && axis === "vertical") tracks.push("auto");
      const before = tracks.length;
      tracks.push(`minmax(0, ${Math.max(0.001, group.share * 100).toFixed(3)}fr)`);
      const content = tracks.length;
      const strip = !tabs ? null : axis === "vertical" ? before : content;
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
   * What the grid holds, in the order it is seen (= the Tab order): a group's strip, the slots of its views, the
   * divider below it. One keyed list: a view moved to another group keeps its slot.
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

  function tabsOf(group: DrawnGroup): TabView[] {
    return group.views.map((id) => {
      const view = arrangement.viewOf(id);
      return {
        id,
        label: arrangement.nameOf(id),
        icon: view?.icon,
        badge: view?.panel?.badge?.() ?? null,
      };
    });
  }

  /** The view a group shows: alone, or its selected tab. */
  function shownIn(group: DrawnGroup): string | null {
    return arrangement.selectedOf(group);
  }

  /** The room a group's view has along the stack now. */
  function sizeOf(group: DrawnGroup): number {
    const box = viewSlots.of(shownIn(group) ?? "")?.getBoundingClientRect();
    return (axis === "vertical" ? box?.height : box?.width) ?? 0;
  }

  /**
   * Where an item sits in the grid: its track along the stack; side by side also its rows (`rows`: the strips' row is
   * the first, the contents' the second).
   */
  function cell(track: number | null, rows: string): { row: string; column: string } {
    const at = String(track ?? 1);
    return axis === "vertical" ? { row: at, column: "1" } : { row: rows, column: at };
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
    viewMenu.at(id, { x: event.clientX, y: event.clientY }, header);
  }

  function headerKeys(id: string, event: KeyboardEvent): void {
    if (!((event.key === "F10" && event.shiftKey) || event.key === "ContextMenu")) return;
    const header = headerOf(event);
    if (header === null || !(event.target instanceof HTMLElement)) return;
    event.preventDefault();
    const box = event.target.getBoundingClientRect();
    viewMenu.at(id, { x: box.left, y: box.bottom + 2 }, event.target);
  }
</script>

<div
  class="groups"
  class:across={axis === "horizontal"}
  data-view-region={region}
  bind:clientHeight={null, (px) => measured("vertical", px)}
  bind:clientWidth={null, (px) => measured("horizontal", px)}
  style:grid-template-rows={axis === "vertical" ? plan.tracks : undefined}
  style:grid-template-columns={axis === "horizontal" ? plan.tracks : undefined}
>
  {#each items as item (item.key)}
    {#if item.kind === "strip"}
      {@const at = cell(item.place.strip, "1")}
      <div
        class="strip"
        style:grid-row={at.row}
        style:grid-column={at.column}
        style:height={`${STRIP_PX}px`}
      >
        <ViewTabs
          views={tabsOf(item.place.group)}
          selected={shownIn(item.place.group)}
          onselect={selectView}
          onmenu={arranging ? (id, at, anchor) => viewMenu.at(id, at, anchor) : undefined}
          label={strips.label}
          roomy={strips.roomy}
        />
      </div>
    {:else if item.kind === "line"}
      {@const top = shownIn(item.place.group) ?? ""}
      {@const bottom = shownIn(item.next.group) ?? ""}
      {@const at = cell(item.place.divider, "1 / -1")}
      <div class="line" style:grid-row={at.row} style:grid-column={at.column}>
        <ViewDivider
          {axis}
          top={arrangement.nameOf(top)}
          bottom={arrangement.nameOf(bottom)}
          controls={`view-${top}`}
          measure={() => ({ above: sizeOf(item.place.group), below: sizeOf(item.next.group) })}
          onsplit={(ratio) =>
            arrangement.arrange(
              withRegion(
                arrangement.layout,
                region,
                resizePair(
                  arrangement.layout[region],
                  item.place.group.at,
                  item.next.group.at,
                  ratio,
                ),
              ),
            )}
        />
      </div>
    {:else}
      {@const id = item.id}
      {@const shown = shownIn(item.place.group) === id}
      <!-- side by side: under its group's strip, or the whole height where the group draws none -->
      {@const at = cell(item.place.content, item.place.strip === null ? "1 / -1" : "2")}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="view"
        class:tabbed={item.place.strip !== null}
        class:off={!shown}
        id={`view-${id}`}
        data-view-slot={id}
        style:grid-row={at.row}
        style:grid-column={at.column}
        inert={!shown}
        oncontextmenu={(event) => headerMenu(id, event)}
        onkeydown={(event) => headerKeys(id, event)}
        {@attach viewSlots.slot(id)}
      ></div>
    {/if}
  {/each}
</div>

<style>
  .groups {
    position: relative;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  /* side by side: the groups' columns come from their shares; a row for the strips over the row of the contents */
  .groups.across {
    grid-template-rows: auto minmax(0, 1fr);
    min-width: 0;
  }
  .strip,
  .line {
    min-width: 0;
    min-height: 0;
  }
  /* the tabs fill the strip's height exactly (it is a whole number of pixels, their own is not) */
  .strip {
    display: grid;
  }
  /* the views of a group share its cell */
  .view {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  /* a tab behind another keeps its room in the cell (its lists keep their scroll offsets): it is not painted and
     nothing in it can be reached */
  .view.off {
    visibility: hidden;
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
</style>
