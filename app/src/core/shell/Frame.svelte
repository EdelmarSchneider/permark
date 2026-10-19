<script lang="ts">
  // Window frame: switcher bar (only with >= 2 workspaces) | header | left sidebar | document area | right panel |
  // status bar. Regions are slots; features fill them (empty slot = region not rendered).
  import type { Snippet } from "svelte";

  type Props = {
    switcher?: Snippet;
    header: Snippet;
    sidebar?: Snippet;
    main: Snippet;
    panel?: Snippet;
    status: Snippet;
  };

  let { switcher, header, sidebar, main, panel, status }: Props = $props();
</script>

<div class="frame" class:has-switcher={switcher !== undefined}>
  {#if switcher}
    <div class="switcher">{@render switcher()}</div>
  {/if}
  <header class="header">{@render header()}</header>
  <div class="body" class:has-sidebar={sidebar !== undefined} class:has-panel={panel !== undefined}>
    {#if sidebar}
      <aside class="sidebar">{@render sidebar()}</aside>
    {/if}
    <main class="main">{@render main()}</main>
    {#if panel}
      <aside class="panel">{@render panel()}</aside>
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
    grid-template-rows: 32px var(--h-header) minmax(0, 1fr) var(--h-status);
  }
  .switcher {
    display: flex;
    align-items: stretch;
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
