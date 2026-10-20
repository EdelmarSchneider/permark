<script lang="ts">
  // The pill near the pointer while files from outside are dragged over the view: what a drop here does. Pointer
  // only (the system's drag): hidden from screen readers; the result is told by the question, a notice or a new tab.
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import ImageIcon from "@lucide/svelte/icons/image";
  import SquarePlus from "@lucide/svelte/icons/square-plus";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import type { Badge } from "../explorer/importModel";
  import { badgePlace } from "./dropPlace";

  type Props = { badge: Badge; at: { x: number; y: number } };
  let { badge, at }: Props = $props();

  let width = $state(0);
  let height = $state(0);
  let viewWidth = $state(typeof window === "undefined" ? 0 : window.innerWidth);
  let viewHeight = $state(typeof window === "undefined" ? 0 : window.innerHeight);

  const place = $derived(
    badgePlace(at, { width, height }, { width: viewWidth, height: viewHeight }),
  );
  const Icon = $derived(
    { import: FolderInput, open: SquarePlus, pictures: ImageIcon, notes: StickyNote }[badge.icon],
  );
</script>

<svelte:window bind:innerWidth={viewWidth} bind:innerHeight={viewHeight} />

<div
  class="badge"
  aria-hidden="true"
  data-icon={badge.icon}
  bind:clientWidth={width}
  bind:clientHeight={height}
  style:transform="translate({place.x}px, {place.y}px)"
>
  <Icon size={14} aria-hidden="true" />
  <span class="text">{badge.text}</span>
</div>

<style>
  .badge {
    position: fixed;
    top: 0;
    left: 0;
    z-index: 70;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    max-width: 320px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    box-shadow: var(--shadow-chip);
    color: var(--muted);
    pointer-events: none;
    animation: pm-fade var(--dur-fast) var(--ease);
  }
  .text {
    overflow: hidden;
    color: var(--text);
    font-size: var(--fs-ui-sm);
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  @media (prefers-reduced-motion: reduce) {
    .badge {
      animation: none;
    }
  }
</style>
