<script lang="ts">
  // What a drag of a view shows, once for the window and only while it runs: a chip with the view's icon and name
  // near the pointer, and the ONE place a drop would put the view (a line between two tabs, a half of a view's
  // body, the edge of a region that is not on screen, named as the menu names that move). Pointer only: hidden from
  // screen readers (the move is said once made; the keyboard's way is the views' menu). Nothing here moves by
  // itself: a place to drop shows and goes at once.
  import { t } from "../i18n";
  import { arrangement } from "./arrangement.svelte";
  import type { DragTarget, Point } from "./dragTarget";
  import { badgePlace } from "./dropPlace";
  import { ACROSS } from "./moveWords";

  type Props = { id: string; at: Point; target: DragTarget | null };
  let { id, at, target }: Props = $props();

  const Icon = $derived(arrangement.viewOf(id)?.icon);
  /** A drop here does something. */
  const over = $derived(target !== null);

  let width = $state(0);
  let height = $state(0);
  let viewWidth = $state(window.innerWidth);
  let viewHeight = $state(window.innerHeight);
  /** The chip beside the pointer, always inside the window. */
  const place = $derived(
    badgePlace(at, { width, height }, { width: viewWidth, height: viewHeight }),
  );
  /** The words of an edge sit in the half of it the pointer is not in (its first half, or its last): never under
   *  the chip. */
  const wordsFirst = $derived.by(() => {
    if (target?.kind !== "region") return false;
    const { left, top, right, bottom } = target.mark;
    return target.region === "bottom" ? at.x > (left + right) / 2 : at.y > (top + bottom) / 2;
  });

  // the whole page while the drag runs (the stylesheet below): no text is selected under the pointer, the cursor
  // says whether a drop here does something
  $effect(() => {
    const root = document.documentElement;
    root.setAttribute("data-view-drag", over ? "target" : "none");
    return () => root.removeAttribute("data-view-drag");
  });
</script>

<svelte:window bind:innerWidth={viewWidth} bind:innerHeight={viewHeight} />

<div class="layer" aria-hidden="true" data-view-drag-layer>
  {#if target}
    {@const { left, top, right, bottom } = target.mark}
    <div
      class="target {target.kind}"
      data-drag-target={target.kind}
      data-drag-region={target.region}
      style:left={`${left}px`}
      style:top={`${top}px`}
      style:width={`${right - left}px`}
      style:height={`${bottom - top}px`}
    >
      {#if target.kind === "region"}
        <span class="words" class:first={wordsFirst}>{t(ACROSS[target.region].label)}</span>
      {/if}
    </div>
  {/if}
  <div
    class="ghost"
    data-drag-ghost
    bind:clientWidth={width}
    bind:clientHeight={height}
    style:transform="translate({place.x}px, {place.y}px)"
  >
    {#if Icon}<Icon size={14} />{/if}
    <span class="name">{arrangement.nameOf(id)}</span>
  </div>
</div>

<style>
  /* over everything the window draws, in the way of nothing: the pointer goes through */
  .layer {
    position: fixed;
    inset: 0;
    z-index: 70;
    overflow: hidden;
    pointer-events: none;
  }
  .target {
    position: fixed;
    box-sizing: border-box;
  }
  /* a place between two tabs */
  .tab {
    border-radius: 1px;
    background: var(--accent);
  }
  /* a half of a view's body, the edge of a region */
  .group,
  .region {
    display: grid;
    place-items: center;
    border: 2px solid var(--accent);
    background: var(--accent-drop);
  }
  .words {
    padding: 2px var(--sp-2);
    border-radius: var(--radius-control);
    background: var(--surface);
    color: var(--text);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
    white-space: nowrap;
  }
  .region[data-drag-region="bottom"] .words {
    justify-self: end;
    margin-inline: var(--sp-3);
  }
  .region[data-drag-region="bottom"] .words.first {
    justify-self: start;
  }
  /* an edge at a side is narrow: its words run down it */
  .region[data-drag-region="left"] .words,
  .region[data-drag-region="right"] .words {
    align-self: end;
    margin-block: var(--sp-3);
    padding: var(--sp-2) 2px;
    writing-mode: vertical-rl;
  }
  .region[data-drag-region="left"] .words.first,
  .region[data-drag-region="right"] .words.first {
    align-self: start;
  }
  .ghost {
    position: fixed;
    top: 0;
    left: 0;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 240px;
    height: 26px;
    padding: 0 var(--sp-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .name {
    overflow: hidden;
    color: var(--text);
    font-weight: 600;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  :global(html[data-view-drag]) {
    user-select: none;
    -webkit-user-select: none;
  }
  :global(html[data-view-drag="none"] *) {
    cursor: not-allowed !important;
  }
  :global(html[data-view-drag="target"] *) {
    cursor: grabbing !important;
  }
</style>
