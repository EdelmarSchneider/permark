<script lang="ts">
  // A small question at the point where files were dropped (not modal: nothing else is blocked). Stays inside the
  // view and does not move once shown; Esc | a click outside closes it; Tab | Shift+Tab stay inside; the element
  // marked `data-autofocus` takes the keyboard when it opens, and the keyboard goes back where it was when it closes.
  import type { Snippet } from "svelte";
  import { popoverPlace } from "./dropPlace";

  type Props = {
    at: { x: number; y: number };
    labelledby: string;
    describedby?: string;
    close: () => void;
    /** The content is complete (a question still checking names has its buttons off). */
    ready?: boolean;
    children: Snippet;
  };
  let { at, labelledby, describedby, close, ready = true, children }: Props = $props();

  let box: HTMLElement | undefined = $state();
  let place = $state<{ x: number; y: number } | null>(null);
  let room = $state<number | null>(null);

  // placed once, from its first size: content that changes later (the plan arriving) never moves it
  $effect(() => {
    if (!box || place !== null) return;
    const rect = box.getBoundingClientRect();
    const view = { width: window.innerWidth, height: window.innerHeight };
    place = popoverPlace(at, { width: rect.width, height: rect.height }, view);
    room = view.height - place.y - 8;
  });

  // the keyboard comes in on open (once the content is there) and goes back on close
  $effect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return () => before?.focus({ preventScroll: true });
  });
  $effect(() => {
    if (place === null || !ready) return;
    const target = box?.querySelector<HTMLElement>("[data-autofocus]:not(:disabled)");
    if (target && !box?.contains(document.activeElement)) target.focus({ preventScroll: true });
  });

  // a press anywhere else closes it (before that press does its own work)
  $effect(() => {
    const outside = (e: PointerEvent): void => {
      if (box && e.target instanceof Node && !box.contains(e.target)) close();
    };
    window.addEventListener("pointerdown", outside, true);
    return () => window.removeEventListener("pointerdown", outside, true);
  });

  /** Esc closes it wherever the keyboard is (also while it checks names and nothing inside can take the keyboard). */
  function onwindowkey(e: KeyboardEvent): void {
    if (e.key !== "Escape") return;
    e.preventDefault();
    close();
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key !== "Tab" || !box) return;
    const focusable = Array.from(
      box.querySelectorAll<HTMLElement>("button:not(:disabled):not([tabindex='-1'])"),
    );
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last?.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first?.focus();
    }
  }
</script>

<svelte:window onkeydown={onwindowkey} />

<div
  bind:this={box}
  class="popover"
  class:placed={place !== null}
  role="dialog"
  aria-labelledby={labelledby}
  aria-describedby={describedby}
  tabindex="-1"
  style:left="{place?.x ?? 0}px"
  style:top="{place?.y ?? 0}px"
  style:max-height={room === null ? undefined : `${room}px`}
  {onkeydown}
>
  {@render children()}
</div>

<style>
  .popover {
    position: fixed;
    z-index: 55;
    width: min(340px, calc(100vw - 16px));
    overflow: auto;
    padding: var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
    outline: none;
    visibility: hidden;
  }
  .popover.placed {
    visibility: visible;
    animation: pm-fade var(--dur-fast) var(--ease);
  }
  @media (prefers-reduced-motion: reduce) {
    .popover.placed {
      animation: none;
    }
  }
</style>
