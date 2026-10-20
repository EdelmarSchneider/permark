<script lang="ts">
  // Window frame: top bar | left sidebar | document area | right panel | status bar. Regions are slots; features fill them (empty slot = region not rendered). The two side panels are
  // resizable: widths come in as props (the layout store keeps them), a splitter on each inner edge asks for changes.
  // Focus mode collapses every region but the page: they stay mounted (state kept) and become inert once faded.
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
    focus = false,
    moving = false,
  }: Props = $props();

  let bodyWidth = $state(0);
  /** What is drawn: the widths asked for, made to fit the window (the page keeps its minimum). */
  const widths = $derived(
    fit(
      bodyWidth,
      sidebar === undefined ? null : sidebarWidth,
      panel === undefined ? null : rightWidth,
    ),
  );
</script>

<div class="frame" class:focus class:moving>
  <header class="top" inert={focus}>{@render top()}</header>
  <div
    class="body"
    class:has-sidebar={sidebar !== undefined}
    class:has-panel={panel !== undefined}
    bind:clientWidth={bodyWidth}
    style:--w-sidebar={widths.sidebar === null ? undefined : `${widths.sidebar}px`}
    style:--w-right={widths.right === null ? undefined : `${widths.right}px`}
  >
    {#if sidebar}
      <aside class="sidebar" inert={focus}>{@render sidebar()}</aside>
    {/if}
    <main class="main">{@render main()}</main>
    {#if panel}
      <aside class="panel" inert={focus}>{@render panel()}</aside>
    {/if}
    {#if sidebar && !focus}
      <Splitter
        side="sidebar"
        range={SIDEBAR}
        value={widths.sidebar ?? sidebarWidth}
        onchange={onSidebarWidth}
      />
    {/if}
    {#if panel && !focus}
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
    border-bottom: 1px solid var(--border);
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

  /* Focus mode: every region but the page collapses and fades; after the fade it is hidden for good (no Tab stop, no
     screen reader text). The tracks transition only just after a toggle, so a splitter drag never animates. */
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
  .frame.focus .body.has-sidebar {
    grid-template-columns: 0px minmax(0, 1fr);
  }
  .frame.focus .body.has-panel {
    grid-template-columns: minmax(0, 1fr) 0px;
  }
  .frame.focus .body.has-sidebar.has-panel {
    grid-template-columns: 0px minmax(0, 1fr) 0px;
  }
  .frame.focus .top,
  .frame.focus .sidebar,
  .frame.focus .panel,
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
    .frame.focus .sidebar,
    .frame.focus .panel,
    .frame.focus .status {
      transition: none;
    }
  }
</style>
