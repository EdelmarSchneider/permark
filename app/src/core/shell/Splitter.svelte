<script lang="ts">
  // The handle on the inner edge of a side panel: drag with the pointer, or focus it and use the arrows (Shift = big
  // steps, Home | End = limits, Enter | double click = default). It only reports a new width; the frame draws it.
  import { t } from "../i18n";
  import { keyWidth, type Range } from "./layout";

  type Props = {
    /** Which panel it belongs to: the sidebar grows to the right, the right panel to the left. */
    side: "sidebar" | "right";
    range: Range;
    /** Width now, as drawn. */
    value: number;
    onchange: (px: number) => void;
  };
  let { side, range, value, onchange }: Props = $props();

  const growsRight = $derived(side === "sidebar");
  let dragging = $state(false);
  let origin = { x: 0, width: 0 };

  function onpointerdown(e: PointerEvent): void {
    if (e.button !== 0) return;
    e.preventDefault();
    origin = { x: e.clientX, width: value };
    dragging = true;
    if (e.currentTarget instanceof Element) e.currentTarget.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent): void {
    if (!dragging) return;
    const moved = e.clientX - origin.x;
    onchange(origin.width + (growsRight ? moved : -moved));
  }

  function onpointerup(e: PointerEvent): void {
    dragging = false;
    if (e.currentTarget instanceof Element) e.currentTarget.releasePointerCapture(e.pointerId);
  }

  function onkeydown(e: KeyboardEvent): void {
    const next = keyWidth(e.key, e.shiftKey, value, range, growsRight);
    if (next === null) return;
    e.preventDefault();
    onchange(next);
  }
</script>

<!-- the window splitter pattern: a focusable separator that takes the arrow keys -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="splitter"
  class:is-dragging={dragging}
  data-side={side}
  role="separator"
  aria-orientation="vertical"
  aria-label={t(side === "sidebar" ? "layout.resizeSidebar" : "layout.resizePanel")}
  aria-valuenow={value}
  aria-valuemin={range.min}
  aria-valuemax={range.max}
  tabindex="0"
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={onpointerup}
  ondblclick={() => onchange(range.def)}
  {onkeydown}
></div>

<style>
  .splitter {
    position: absolute;
    top: 0;
    bottom: 0;
    z-index: 5;
    width: 7px;
    cursor: col-resize;
    touch-action: none;
    outline: none;
  }
  /* centered on the border of its panel: the frame's body carries the widths as variables */
  .splitter[data-side="sidebar"] {
    left: calc(var(--w-sidebar) - 4px);
  }
  .splitter[data-side="right"] {
    right: calc(var(--w-right) - 3px);
  }
  /* the line appears on hover | focus | drag, centered on the border */
  .splitter::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 3px;
    width: 2px;
    background: transparent;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .splitter:hover::after,
  .splitter:focus-visible::after,
  .splitter.is-dragging::after {
    background: var(--accent-muted);
  }
  .splitter:focus-visible::after {
    background: var(--accent);
  }
</style>
