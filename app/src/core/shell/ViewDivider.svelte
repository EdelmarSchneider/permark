<script lang="ts">
  // The line between two groups of a region: drag it, or focus it and use the arrows along the stack (Up | Down for
  // groups one above the other, Left | Right for groups side by side; Shift = big steps, Home | End = one view at its
  // smallest, Enter | double click = half each). It only reports the new split; the region draws it (window splitter
  // pattern: the bar lies across the stack).
  import { t } from "../i18n";
  import { STEP, STEP_BIG } from "./layout";
  import { MIN_GROUP_PX, type Axis } from "./arrangement";

  type Props = {
    /** The views before | after it along the stack: above | below, or left | right (their tab labels). */
    top: string;
    bottom: string;
    /** The element of the view before it (`aria-controls`). */
    controls: string;
    /** Sizes now along the stack of the groups before | after it, as drawn. */
    measure: () => { above: number; below: number };
    /** The new split: the share of both sizes the group before it takes (0..1). */
    onsplit: (ratio: number) => void;
    /** The way the region stacks its groups (default: one above the other). */
    axis?: Axis;
  };
  let { top, bottom, controls, measure, onsplit, axis = "vertical" }: Props = $props();

  /** Per stack: the keys that move the bar back | on, the bar's own direction. */
  const STACKS = {
    vertical: { back: "ArrowUp", on: "ArrowDown", bar: "horizontal" },
    horizontal: { back: "ArrowLeft", on: "ArrowRight", bar: "vertical" },
  } as const;

  const of = $derived(STACKS[axis]);
  let dragging = $state(false);
  let origin = { at: 0, above: 0, both: 0 };
  /** The split now, for assistive tech (re-read after every change). */
  let percent = $state(50);
  let bounds = $state({ min: 0, max: 100 });

  /** Where the pointer is along the stack. */
  function along(e: PointerEvent): number {
    return axis === "vertical" ? e.clientY : e.clientX;
  }

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

  /** The group before it at `px` (kept inside both minimums). */
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
    origin = { at: along(e), above, both: above + below };
    dragging = true;
    if (e.currentTarget instanceof Element) e.currentTarget.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent): void {
    if (!dragging) return;
    split(origin.above + along(e) - origin.at, origin.both);
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
      case of.back:
        split(above - step, both);
        break;
      case of.on:
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
  aria-orientation={of.bar}
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
  /* the whole divider track (7 px) is the grab zone; the 1 px line sits in its middle; nothing of a view under it */
  .divider {
    position: relative;
    height: 100%;
    cursor: row-resize;
    touch-action: none;
    outline: none;
  }
  .divider[aria-orientation="vertical"] {
    cursor: col-resize;
  }
  .divider::after {
    content: "";
    position: absolute;
    background: var(--border);
  }
  .divider[aria-orientation="horizontal"]::after {
    top: 3px;
    right: 0;
    left: 0;
    height: 1px;
  }
  .divider[aria-orientation="vertical"]::after {
    top: 0;
    bottom: 0;
    left: 3px;
    width: 1px;
  }
  .divider:hover::after,
  .divider.is-dragging::after {
    background: var(--accent-muted);
  }
  .divider:focus-visible::after {
    background: var(--accent);
  }
  .divider[aria-orientation="horizontal"]:focus-visible::after {
    top: 1px;
    height: 5px;
  }
  .divider[aria-orientation="vertical"]:focus-visible::after {
    left: 1px;
    width: 5px;
  }
</style>
