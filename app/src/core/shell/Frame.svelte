<script lang="ts">
  // Window frame: top bar | left sidebar | center column (the document area over the bottom panel) | right panel |
  // status bar. Regions are slots; features fill them (empty slot = region not rendered). The panels are resizable:
  // their sizes come in as props (the layout store keeps them), a splitter on each inner edge asks for changes.
  // Each panel can be hidden on its own (the person's choice); focus mode collapses every region but the page. A
  // hidden region stays mounted (state kept) and becomes inert once faded; the page takes its room.
  import type { Snippet } from "svelte";
  import { t } from "../i18n";
  import { BOTTOM, fit, PAGE_MIN_H, RIGHT, SIDEBAR, type Range } from "./layout";
  import Splitter from "./Splitter.svelte";

  type Props = {
    top: Snippet;
    sidebar?: Snippet;
    main: Snippet;
    /** The bottom panel: under the page, between the side panels. */
    bottom?: Snippet;
    panel?: Snippet;
    status: Snippet;
    sidebarWidth?: number;
    rightWidth?: number;
    /** The bottom panel's height as it is drawn: its owner made it fit the height reported by `onCenterHeight`. */
    bottomHeight?: number;
    onSidebarWidth?: (px: number) => void;
    onRightWidth?: (px: number) => void;
    onBottomHeight?: (px: number) => void;
    /** The center column (the page over the bottom panel) is this high now: what the bottom panel has to fit in. */
    onCenterHeight?: (px: number) => void;
    /** The sidebar | the right panel | the bottom panel is off: the person hid it (the bottom one also: the window
     *  is too short for it). */
    sidebarHidden?: boolean;
    rightHidden?: boolean;
    bottomHidden?: boolean;
    /** Focus mode: only the page. */
    focus?: boolean;
    /** The moment of a toggle: the tracks animate now (never while a splitter is dragged). */
    moving?: boolean;
  };

  let {
    top,
    sidebar,
    main,
    bottom,
    panel,
    status,
    sidebarWidth = SIDEBAR.def,
    rightWidth = RIGHT.def,
    bottomHeight = BOTTOM.def,
    onSidebarWidth = () => {},
    onRightWidth = () => {},
    onBottomHeight = () => {},
    onCenterHeight = () => {},
    sidebarHidden = false,
    rightHidden = false,
    bottomHidden = false,
    focus = false,
    moving = false,
  }: Props = $props();

  /** On screen: there, not hidden by the person, not folded by focus mode. */
  const sideOpen = $derived(sidebar !== undefined && !sidebarHidden && !focus);
  const rightOpen = $derived(panel !== undefined && !rightHidden && !focus);

  let bodyWidth = $state(0);
  /** What is drawn: the widths asked for, made to fit the window (the page keeps its minimum; a hidden panel takes
   *  no room). */
  const widths = $derived(
    fit(bodyWidth, sideOpen ? sidebarWidth : null, rightOpen ? rightWidth : null),
  );

  let centerHeight = $state(0);
  /** The bottom panel's height as drawn; `null`: off (hidden, a window too short for it) or folded by focus mode. */
  const bottomDrawn = $derived(
    bottom !== undefined && !bottomHidden && !focus ? bottomHeight : null,
  );
  /** The heights its splitter can ask for: no taller than the window gives beside the page's minimum. */
  const bottomRange = $derived<Range>({
    ...BOTTOM,
    max: centerHeight > 0 ? Math.max(BOTTOM.min, centerHeight - PAGE_MIN_H) : BOTTOM.max,
  });

  /** The track of a panel: its size as drawn, 0 while it is not (`null`); the track animates just after a toggle. */
  function track(present: boolean, size: number | null): string | undefined {
    if (!present) return undefined;
    return size !== null ? `${size}px` : "0px";
  }
</script>

<div class="frame" class:focus class:moving>
  <header class="top" inert={focus}>{@render top()}</header>
  <div
    class="body"
    data-frame="body"
    class:has-sidebar={sidebar !== undefined}
    class:has-panel={panel !== undefined}
    bind:clientWidth={bodyWidth}
    style:--w-sidebar={track(sidebar !== undefined, widths.sidebar)}
    style:--w-right={track(panel !== undefined, widths.right)}
  >
    {#if sidebar}
      <aside
        class="sidebar"
        class:gone={!sideOpen}
        data-region="sidebar"
        aria-label={t("panel.sidebar")}
        inert={!sideOpen}
      >
        {@render sidebar()}
      </aside>
    {/if}
    <!-- the page over the bottom panel, between the side panels (they keep the whole height) -->
    <div
      class="center"
      data-frame="center"
      class:has-bottom={bottom !== undefined}
      bind:clientHeight={
        null,
        (px) => {
          centerHeight = px ?? 0;
          onCenterHeight(centerHeight);
        }
      }
      style:--h-bottom={track(bottom !== undefined, bottomDrawn)}
    >
      <main class="main" data-region="page" tabindex="-1">{@render main()}</main>
      {#if bottom}
        {#if bottomDrawn !== null}
          <Splitter
            side="bottom"
            range={bottomRange}
            value={bottomDrawn}
            onchange={(px) => onBottomHeight(Math.min(px, bottomRange.max))}
          />
        {/if}
        <aside
          class="bottom"
          class:gone={bottomDrawn === null}
          data-region="bottom"
          aria-label={t("panel.bottom")}
          inert={bottomDrawn === null}
        >
          {@render bottom()}
        </aside>
      {/if}
    </div>
    {#if panel}
      <aside
        class="panel"
        class:gone={!rightOpen}
        data-region="right"
        aria-label={t("panel.right")}
        inert={!rightOpen}
      >
        {@render panel()}
      </aside>
    {/if}
    {#if sidebar && sideOpen}
      <Splitter
        side="sidebar"
        range={SIDEBAR}
        value={widths.sidebar ?? sidebarWidth}
        onchange={onSidebarWidth}
      />
    {/if}
    {#if panel && rightOpen}
      <Splitter
        side="right"
        range={RIGHT}
        value={widths.right ?? rightWidth}
        onchange={onRightWidth}
      />
    {/if}
  </div>
  <footer class="status" inert={focus}>{@render status()}</footer>
</div>

<style>
  .frame {
    display: grid;
    grid-template-rows: var(--h-top) minmax(0, 1fr) var(--h-status);
    height: 100%;
  }
  .top {
    display: flex;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--surface);
    /* the bottom line is drawn inside the band, so the active tab can cover it (it joins the page below) */
    box-shadow: inset 0 -1px 0 var(--border);
  }
  .body {
    position: relative;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    min-height: 0;
  }
  .body.has-sidebar {
    grid-template-columns: var(--w-sidebar) minmax(0, 1fr);
  }
  .body.has-panel {
    grid-template-columns: minmax(0, 1fr) var(--w-right);
  }
  .body.has-sidebar.has-panel {
    grid-template-columns: var(--w-sidebar) minmax(0, 1fr) var(--w-right);
  }
  .center {
    position: relative;
    display: grid;
    grid-template-rows: minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr);
    min-width: 0;
    min-height: 0;
  }
  .center.has-bottom {
    grid-template-rows: minmax(0, 1fr) var(--h-bottom);
  }
  .sidebar,
  .panel,
  .bottom {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--surface);
  }
  .sidebar {
    border-right: 1px solid var(--border);
  }
  .panel {
    border-left: 1px solid var(--border);
  }
  .bottom {
    border-top: 1px solid var(--border);
  }
  /* nothing of the page (a header too wide for it) ever paints over the panels beside it */
  .main {
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    overflow: clip;
    background: var(--bg);
  }
  .main:focus {
    outline: none;
  }
  .status {
    display: flex;
    min-height: 0;
    align-items: center;
    gap: var(--sp-3);
    min-width: 0;
    overflow: hidden;
    padding: 0 var(--sp-3);
    font-size: var(--fs-ui-xs);
    color: var(--muted);
    background: var(--surface);
    /* the top line is drawn inside the band: the full 24 px stay for the controls (24 px targets) */
    box-shadow: inset 0 1px 0 var(--border);
  }

  /* Focus mode: every region but the page collapses and fades; a panel the person hid does the same. After the
     fade a region is hidden for good (no Tab stop, no screen reader text). The tracks transition only just after a
     toggle, so a splitter drag never animates. */
  .top,
  .sidebar,
  .panel,
  .bottom,
  .status {
    transition:
      opacity var(--dur) var(--ease),
      visibility 0s;
  }
  .frame.moving {
    transition: grid-template-rows var(--dur) var(--ease);
  }
  .frame.moving .body {
    transition: grid-template-columns var(--dur) var(--ease);
  }
  .frame.moving .center {
    transition: grid-template-rows var(--dur) var(--ease);
  }
  .frame.focus {
    grid-template-rows: 0px minmax(0, 1fr) 0px;
  }
  .frame.focus .top,
  .sidebar.gone,
  .panel.gone,
  .bottom.gone,
  .frame.focus .status {
    border-width: 0;
    opacity: 0;
    visibility: hidden;
    transition:
      opacity var(--dur) var(--ease),
      visibility 0s linear var(--dur);
  }
  @media (prefers-reduced-motion: reduce) {
    .frame,
    .frame.moving,
    .frame.moving .body,
    .frame.moving .center,
    .top,
    .sidebar,
    .panel,
    .bottom,
    .status,
    .frame.focus .top,
    .sidebar.gone,
    .panel.gone,
    .bottom.gone,
    .frame.focus .status {
      transition: none;
    }
  }
</style>
