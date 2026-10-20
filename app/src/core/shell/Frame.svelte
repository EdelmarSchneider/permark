<script lang="ts">
  // Window frame: top bar | left sidebar | document area | right panel | status bar. Regions are slots; features fill them (empty slot = region not rendered). The two side panels are
  // resizable: widths come in as props (the layout store keeps them), a splitter on each inner edge asks for changes.
  // Each side panel can be hidden on its own (the person's choice); focus mode collapses every region but the page. A
  // hidden region stays mounted (state kept) and becomes inert once faded; the page takes its room.
  import type { Snippet } from "svelte";
  import { fit, RIGHT, SIDEBAR } from "./layout";
  import Splitter from "./Splitter.svelte";

  type Props = {
    top: Snippet;
    sidebar?: Snippet;
    main: Snippet;
    panel?: Snippet;
    status: Snippet;
    sidebarWidth?: number;
    rightWidth?: number;
    onSidebarWidth?: (px: number) => void;
    onRightWidth?: (px: number) => void;
    /** The person hid the sidebar | the right panel. */
    sidebarHidden?: boolean;
    rightHidden?: boolean;
    /** Focus mode: only the page. */
    focus?: boolean;
    /** The moment of a toggle: the tracks animate now (never while a splitter is dragged). */
    moving?: boolean;
  };

  let {
    top,
    sidebar,
    main,
    panel,
    status,
    sidebarWidth = SIDEBAR.def,
    rightWidth = RIGHT.def,
    onSidebarWidth = () => {},
    onRightWidth = () => {},
    sidebarHidden = false,
    rightHidden = false,
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
  /** The track of a panel: its width, 0 while hidden (the track animates just after a toggle). */
  function track(present: boolean, open: boolean, width: number | null): string | undefined {
    if (!present) return undefined;
    return open && width !== null ? `${width}px` : "0px";
  }
</script>

<div class="frame" class:focus class:moving>
  <header class="top" inert={focus}>{@render top()}</header>
  <div
    class="body"
    class:has-sidebar={sidebar !== undefined}
    class:has-panel={panel !== undefined}
    bind:clientWidth={bodyWidth}
    style:--w-sidebar={track(sidebar !== undefined, sideOpen, widths.sidebar)}
    style:--w-right={track(panel !== undefined, rightOpen, widths.right)}
  >
    {#if sidebar}
      <aside class="sidebar" class:gone={!sideOpen} data-region="sidebar" inert={!sideOpen}>
        {@render sidebar()}
      </aside>
    {/if}
    <main class="main">{@render main()}</main>
    {#if panel}
      <aside class="panel" class:gone={!rightOpen} data-region="right" inert={!rightOpen}>
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
  .sidebar,
  .panel {
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
    border-top: 1px solid var(--border);
  }

  /* Focus mode: every region but the page collapses and fades; a side panel the person hid does the same. After the
     fade a region is hidden for good (no Tab stop, no screen reader text). The tracks transition only just after a
     toggle, so a splitter drag never animates. */
  .top,
  .sidebar,
  .panel,
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
  .frame.focus {
    grid-template-rows: 0px minmax(0, 1fr) 0px;
  }
  .frame.focus .top,
  .sidebar.gone,
  .panel.gone,
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
    .top,
    .sidebar,
    .panel,
    .status,
    .frame.focus .top,
    .sidebar.gone,
    .panel.gone,
    .frame.focus .status {
      transition: none;
    }
  }
</style>
