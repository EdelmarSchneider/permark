<script lang="ts">
  // Window frame: switcher bar (only with >= 2 workspaces) | header | left sidebar | document area | right panel |
  // status bar. Regions are slots; features fill them (empty slot = region not rendered). The two side panels are
  // resizable: widths come in as props (the layout store keeps them), a splitter on each inner edge asks for changes.
  import type { Snippet } from "svelte";
  import { fit, RIGHT, SIDEBAR } from "./layout";
  import Splitter from "./Splitter.svelte";

  type Props = {
    switcher?: Snippet;
    header: Snippet;
    sidebar?: Snippet;
    main: Snippet;
    panel?: Snippet;
    status: Snippet;
    sidebarWidth?: number;
    rightWidth?: number;
    onSidebarWidth?: (px: number) => void;
    onRightWidth?: (px: number) => void;
  };

  let {
    switcher,
    header,
    sidebar,
    main,
    panel,
    status,
    sidebarWidth = SIDEBAR.def,
    rightWidth = RIGHT.def,
    onSidebarWidth = () => {},
    onRightWidth = () => {},
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

<div class="frame" class:has-switcher={switcher !== undefined}>
  {#if switcher}
    <div class="switcher">{@render switcher()}</div>
  {/if}
  <header class="header">{@render header()}</header>
  <div
    class="body"
    class:has-sidebar={sidebar !== undefined}
    class:has-panel={panel !== undefined}
    bind:clientWidth={bodyWidth}
    style:--w-sidebar={widths.sidebar === null ? undefined : `${widths.sidebar}px`}
    style:--w-right={widths.right === null ? undefined : `${widths.right}px`}
  >
    {#if sidebar}
      <aside class="sidebar">{@render sidebar()}</aside>
    {/if}
    <main class="main">{@render main()}</main>
    {#if panel}
      <aside class="panel">{@render panel()}</aside>
    {/if}
    {#if sidebar}
      <Splitter
        side="sidebar"
        range={SIDEBAR}
        value={widths.sidebar ?? sidebarWidth}
        onchange={onSidebarWidth}
      />
    {/if}
    {#if panel}
      <Splitter
        side="right"
        range={RIGHT}
        value={widths.right ?? rightWidth}
        onchange={onRightWidth}
      />
    {/if}
  </div>
  <footer class="status">{@render status()}</footer>
</div>

<style>
  .frame {
    display: grid;
    grid-template-rows: var(--h-header) minmax(0, 1fr) var(--h-status);
    height: 100%;
  }
  .frame.has-switcher {
    grid-template-rows: var(--h-switcher) var(--h-header) minmax(0, 1fr) var(--h-status);
  }
  .switcher {
    min-width: 0;
    overflow: hidden;
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
  }
  .header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
    padding: 0 var(--sp-3);
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
  .main {
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }
  .status {
    display: flex;
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
</style>
