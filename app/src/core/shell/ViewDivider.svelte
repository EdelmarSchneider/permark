<script lang="ts">
  // The line between two groups of the right column: drag it, or focus it and use Up | Down (Shift = big steps, Home |
  // End = one view at its smallest, Enter | double click = half each). It only reports the new split; the column
  // draws it (window splitter pattern, horizontal).
  import { t } from "../i18n";
  import { STEP, STEP_BIG } from "./layout";
  import { MIN_GROUP_PX } from "./rightViews";

  type Props = {
    /** The views above | below (their tab labels). */
    top: string;
    bottom: string;
    /** The element of the view above (`aria-controls`). */
    controls: string;
    /** Heights now of the groups above | below, as drawn. */
    measure: () => { above: number; below: number };
    /** The new split: the share of both heights the group above takes (0..1). */
    onsplit: (ratio: number) => void;
  };
  let { top, bottom, controls, measure, onsplit }: Props = $props();

  let dragging = $state(false);
  let origin = { y: 0, above: 0, both: 0 };
  /** The split now, for assistive tech (re-read after every change). */
  let percent = $state(50);
  let bounds = $state({ min: 0, max: 100 });

  function sync(): void {
    const { above, below } = measure();
    const both = above + below;
    if (both <= 0) return;
    percent = Math.round((above / both) * 100);
    bounds = {
      min: Math.round((Math.min(MIN_GROUP_PX, both / 2) / both) * 100),
      max: Math.round(((both - Math.min(MIN_GROUP_PX, both / 2)) / both) * 100),
    };
  }

  /** The group above at `px` (kept inside both minimums). */
  function split(px: number, both: number): void {
    if (both <= 0) return;
    const least = Math.min(MIN_GROUP_PX, both / 2);
    const above = Math.min(both - least, Math.max(least, px));
    onsplit(above / both);
    percent = Math.round((above / both) * 100);
  }

  function onpointerdown(e: PointerEvent): void {
    if (e.button !== 0) return;
    e.preventDefault();
    const { above, below } = measure();
    origin = { y: e.clientY, above, both: above + below };
    dragging = true;
    if (e.currentTarget instanceof Element) e.currentTarget.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent): void {
    if (!dragging) return;
    split(origin.above + e.clientY - origin.y, origin.both);
  }

  function onpointerup(e: PointerEvent): void {
    dragging = false;
    if (e.currentTarget instanceof Element && e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    const { above, below } = measure();
    const both = above + below;
    const step = e.shiftKey ? STEP_BIG : STEP;
    switch (e.key) {
      case "ArrowUp":
        split(above - step, both);
        break;
      case "ArrowDown":
        split(above + step, both);
        break;
      case "Home":
        split(0, both);
        break;
      case "End":
        split(both, both);
        break;
      case "Enter":
        split(both / 2, both);
        break;
      default:
        return;
    }
    e.preventDefault();
  }
</script>

<!-- the window splitter pattern: a focusable separator that takes the arrow keys -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="divider"
  class:is-dragging={dragging}
  role="separator"
  aria-orientation="horizontal"
  aria-controls={controls}
  aria-label={t("layout.resizeViews", { top, bottom })}
  aria-valuenow={percent}
  aria-valuemin={bounds.min}
  aria-valuemax={bounds.max}
  aria-valuetext={t("layout.viewsValue", { top, percent })}
  tabindex="0"
  onfocus={sync}
  onpointerenter={sync}
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={onpointerup}
  ondblclick={() => {
    const { above, below } = measure();
    split((above + below) / 2, above + below);
  }}
  {onkeydown}
></div>

<style>
  /* a 1 px line in the column; the hit zone (8 px) reaches over the views around it */
  .divider {
    position: relative;
    z-index: 2;
    height: 1px;
    background: var(--border);
    cursor: row-resize;
    touch-action: none;
    outline: none;
  }
  .divider::before {
    content: "";
    position: absolute;
    inset: -4px 0 -3px;
  }
  .divider:hover,
  .divider.is-dragging {
    background: var(--accent-muted);
  }
  .divider:focus-visible {
    background: var(--accent);
    box-shadow: 0 0 0 2px var(--accent);
  }
</style>
