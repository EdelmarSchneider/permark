<script lang="ts">
  // The handle on the inner edge of a panel: drag with the pointer, or focus it and use the arrows (Shift = big
  // steps, Home | End = limits, Enter | double click = default). It only reports a new size; the frame draws it.
  import { t } from "../i18n";
  import { keyWidth, type Range } from "./layout";

  type Props = {
    /** Which panel it belongs to: the sidebar grows to the right, the right panel to the left, the bottom panel up. */
    side: "sidebar" | "right" | "bottom";
    range: Range;
    /** Size now, as drawn: a side panel's width, the bottom panel's height. */
    value: number;
    onchange: (px: number) => void;
  };
  let { side, range, value, onchange }: Props = $props();

  /** Per edge: the way its panel grows, its name, the bar's direction (the bottom panel's lies across the window). */
  const SIDES = {
    sidebar: { grows: "right", label: "layout.resizeSidebar", bar: "vertical" },
    right: { grows: "left", label: "layout.resizePanel", bar: "vertical" },
    bottom: { grows: "up", label: "layout.resizeBottom", bar: "horizontal" },
  } as const;

  const of = $derived(SIDES[side]);
  let dragging = $state(false);
  let origin = { at: 0, size: 0 };

  /** Where the pointer is along the way the panel grows | shrinks. */
  function along(e: PointerEvent): number {
    return of.grows === "up" ? e.clientY : e.clientX;
  }

  function onpointerdown(e: PointerEvent): void {
    if (e.button !== 0) return;
    e.preventDefault();
    origin = { at: along(e), size: value };
    dragging = true;
    if (e.currentTarget instanceof Element) e.currentTarget.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent): void {
    if (!dragging) return;
    const moved = along(e) - origin.at;
    onchange(origin.size + (of.grows === "right" ? moved : -moved));
  }

  function onpointerup(e: PointerEvent): void {
    dragging = false;
    if (e.currentTarget instanceof Element) e.currentTarget.releasePointerCapture(e.pointerId);
  }

  function onkeydown(e: KeyboardEvent): void {
    const next = keyWidth(e.key, e.shiftKey, value, range, of.grows);
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
  aria-orientation={of.bar}
  aria-label={t(of.label)}
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
    z-index: 5;
    touch-action: none;
    outline: none;
  }
  .splitter[aria-orientation="vertical"] {
    top: 0;
    bottom: 0;
    width: 7px;
    cursor: col-resize;
  }
  /* centered on the border of its panel: the frame's body carries the widths as variables */
  .splitter[data-side="sidebar"] {
    left: calc(var(--w-sidebar) - 4px);
  }
  .splitter[data-side="right"] {
    right: calc(var(--w-right) - 3px);
  }
  /* the bottom panel's: across the center column (which carries the height), centered on the panel's top border */
  .splitter[data-side="bottom"] {
    right: 0;
    bottom: calc(var(--h-bottom) - 3px);
    left: 0;
    height: 7px;
    cursor: row-resize;
  }
  /* the line appears on hover | focus | drag, centered on the border */
  .splitter::after {
    content: "";
    position: absolute;
    background: transparent;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .splitter[aria-orientation="vertical"]::after {
    top: 0;
    bottom: 0;
    left: 3px;
    width: 2px;
  }
  .splitter[aria-orientation="horizontal"]::after {
    top: 3px;
    right: 0;
    left: 0;
    height: 2px;
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
