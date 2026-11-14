<script lang="ts">
  // The views of this window, each created ONCE: Files, Search, the outline, the panels plugins add. A view lives in
  // a host element that is moved into the slot its region made for it (`ViewRegion`, `viewSlots`), and back home
  // while no region draws it. A view that changes group | region is moved, never created again: what was typed, the
  // rows that are open, the filters stay; the scroll offsets a move loses are put back. Also drawn here, once for the
  // window: the views' options menu and the line that says what a move did.
  //
  // MUST come before the frame in the window's template: the offsets are noted before the regions draw a change.
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import { untrack, type Component } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import type { OutlineItem } from "../../api";
  import OutlinePanel from "../doc/OutlinePanel.svelte";
  import ContextMenu from "../explorer/ContextMenu.svelte";
  import ExplorerSidebar from "../explorer/ExplorerSidebar.svelte";
  import { t } from "../i18n";
  import type { PanelViewProps } from "../plugins/props";
  import SearchPanel from "../search/SearchPanel.svelte";
  import type { OnScreen, WindowStore } from "../window.svelte";
  import { FILES, OUTLINE, REGIONS, SEARCH } from "./arrangement";
  import { arrangement } from "./arrangement.svelte";
  import { viewMenu } from "./viewMenu.svelte";
  import { viewSlots } from "./viewSlots.svelte";

  type Props = {
    store: WindowStore;
    /** What the window has on screen: the project the Explorer and the Search belong to, its document. */
    shown: OnScreen | null;
    /** The outline of the document shown (`null`: it has none). */
    outline: readonly OutlineItem[] | null;
  };
  let { store, shown, outline }: Props = $props();

  /** The present views, by id: one host each. Ids, not view records: a host is never set up twice for one view. */
  const ids = $derived(arrangement.present.map((view) => view.id));
  /** The project on screen: the views that exist only in a project are made anew for each one. */
  const project = $derived(shown?.workspace.kind === "project" ? shown.workspace : null);
  /** Which view sits in which group of which region: when it changes, the regions move slots in the page. */
  const placement = $derived(
    JSON.stringify(REGIONS.map((region) => arrangement.drawn(region).map((group) => group.views))),
  );

  /** Each view's host and the element it waits in while no region draws it. */
  const hosts = new Map<string, { host: HTMLElement; home: HTMLElement }>();
  /** Scroll offsets noted before the regions draw a change; each host puts its own back once it is in a slot. */
  const offsets = new Map<Element, { top: number; left: number }>();

  /** `node` becomes the last child of `parent`. Where the webview can move a connected element without taking it
   *  out of the page (`moveBefore`), the focus and running animations inside it survive. */
  function moveInto(parent: Element, node: Element): void {
    if ("moveBefore" in parent && parent.isConnected && node.isConnected) {
      parent.moveBefore(node, null);
    } else {
      parent.appendChild(node);
    }
  }

  /** The offsets noted for the lists inside `host` are theirs again. */
  function putBack(host: HTMLElement): void {
    for (const [el, at] of offsets) {
      if (!host.contains(el)) continue;
      el.scrollTop = at.top;
      el.scrollLeft = at.left;
      offsets.delete(el);
    }
  }

  // before the regions draw a change of the placement: a move in the page (a slot that changes place, a view that
  // changes slot) resets the scroll offsets of what it holds, so they are noted now. A view about to leave its
  // region waits at home meanwhile: still in the page when its old slot is taken out of it.
  $effect.pre(() => {
    void placement;
    untrack(() => {
      offsets.clear();
      for (const [id, { host, home }] of hosts) {
        for (const el of host.querySelectorAll("*")) {
          if (el.scrollTop !== 0 || el.scrollLeft !== 0) {
            offsets.set(el, { top: el.scrollTop, left: el.scrollLeft });
          }
        }
        const at = host.closest("[data-view-region]")?.getAttribute("data-view-region") ?? null;
        if (at !== null && at !== arrangement.regionOf(id)) moveInto(home, host);
      }
    });
  });

  const placers = new Map<string, Attachment<HTMLElement>>();

  /**
   * For the host of `id` (`{@attach}`): it follows its slot (into it when a region draws the view, back home when
   * the slot goes), gets its scroll offsets back after each change of the placement, and leaves the page with its
   * view. One function per id: the host is never set up again while its view lives.
   */
  function placed(id: string): Attachment<HTMLElement> {
    let attachment = placers.get(id);
    if (attachment === undefined) {
      attachment = (host) => {
        const home = host.parentElement;
        if (home === null) return;
        hosts.set(id, { host, home });
        $effect(() => {
          void placement;
          const slot = viewSlots.of(id);
          const target = slot ?? home;
          if (host.parentElement !== target) moveInto(target, host);
          if (slot !== undefined) putBack(host);
        });
        return () => {
          hosts.delete(id);
          host.remove();
        };
      };
      placers.set(id, attachment);
    }
    return attachment;
  }
</script>

<div hidden data-view-homes>
  {#each ids as id (id)}
    {@const view = arrangement.viewOf(id)}
    {@const panel = view?.panel ?? null}
    {#snippet options()}
      <button
        type="button"
        class="options"
        data-view-menu={id}
        aria-label={t("panel.viewMenu", { name: arrangement.nameOf(id) })}
        title={t("panel.viewMenu", { name: arrangement.nameOf(id) })}
        aria-haspopup="menu"
        aria-expanded={viewMenu.open?.id === id}
        onclick={(event) => viewMenu.toggle(id, event.currentTarget)}
      >
        <Ellipsis size={14} aria-hidden="true" />
      </button>
    {/snippet}
    <!-- a view with a header of its own shows the button there while the window has views to arrange; as a tab
         of a group it has none: the group's strip has the one, for the tab it shows -->
    {@const group = arrangement.groupWith(id)}
    {@const menu =
      view?.ownHeader === true &&
      viewMenu.hasButton &&
      !(group !== undefined && arrangement.hasStrip(group))
        ? options
        : undefined}
    {#snippet body()}
      {#if id === FILES}
        <ExplorerSidebar {store} document={shown?.document ?? null} />
      {:else if id === SEARCH}
        {#if project}<SearchPanel root={project.root} />{/if}
      {:else if id === OUTLINE}
        {#if outline}<OutlinePanel items={outline} viewMenu={menu} />{/if}
      {:else if panel}
        {#await panel.view() then loaded}
          {@const PanelView = loaded.default as unknown as Component<PanelViewProps>}
          <PanelView viewMenu={menu} />
        {/await}
      {/if}
    {/snippet}
    <div data-view-home={id}>
      <div class="host" data-view-host={id} {@attach placed(id)}>
        {#if view?.closable === false}
          <!-- a view of the project (Files, Search, a plugin's sidebar tab): a new one for each project shown -->
          {#if project}
            {#key project.id}{@render body()}{/key}
          {/if}
        {:else}
          {@render body()}
        {/if}
      </div>
    </div>
  {/each}
</div>

{#if viewMenu.open}
  {@const open = viewMenu.open}
  <ContextMenu
    x={open.x}
    y={open.y}
    entries={viewMenu.entries(open.id)}
    label={t("panel.viewMenu", { name: arrangement.nameOf(open.id) })}
    anchor={open.anchor}
    close={() => viewMenu.close()}
  />
{/if}
<p class="sr-only" role="status" data-view-status>{arrangement.status}</p>

<style>
  /* fills the slot it is moved into, as the view's body did when the region drew it */
  .host {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
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
