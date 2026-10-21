<script lang="ts">
  // A small question at the point where files were dropped (not modal: nothing else is blocked). Stays inside the
  // view and does not move once shown; Esc | a click outside closes it; Tab | Shift+Tab stay inside; the element
  // marked `data-autofocus` takes the keyboard when it opens, and the keyboard goes back where it was when it closes.
  // A tab switch is no answer: the question waits in its tab (the view is hidden, its state kept) and takes the
  // keyboard again when the tab comes back.
  import { tick, type Snippet } from "svelte";
  import { onScreen } from "../inFront.svelte";
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
  // after the placed box is drawn (hidden until placed: focus() on it before would do nothing); while it checks names
  // the box itself holds the keyboard (Esc, Tab stay with it), then its first answer. Only while the view is on screen
  // (a hidden tab never takes the keyboard); again when it comes back.
  $effect(() => {
    if (place === null || !onScreen()) return;
    const want = ready;
    let live = true;
    void tick().then(() => {
      if (!live || !box) return;
      const target = want ? box.querySelector<HTMLElement>("[data-autofocus]:not(:disabled)") : box;
      if (!target || target === document.activeElement) return;
      if (target !== box && box.contains(document.activeElement) && document.activeElement !== box)
        return;
      target.focus({ preventScroll: true });
    });
    return () => {
      live = false;
    };
  });

  // a press anywhere else closes it (before that press does its own work); a press on another tab's chip is a tab
  // switch, not an answer
  $effect(() => {
    const outside = (e: PointerEvent): void => {
      if (!box || !(e.target instanceof Node) || box.contains(e.target)) return;
      if (e.target instanceof Element && e.target.closest("[data-tab]:not([aria-current])")) return;
      close();
    };
    window.addEventListener("pointerdown", outside, true);
    return () => window.removeEventListener("pointerdown", outside, true);
  });

  /** Esc closes it wherever the keyboard is (also while it checks names and nothing inside can take the keyboard). */
  function onwindowkey(e: KeyboardEvent): void {
    if (e.key !== "Escape" || !onScreen()) return;
    e.preventDefault();
    close();
  }

  function onkeydown(e: KeyboardEvent): void {
    // Ctrl | Cmd | Alt + Tab switch tabs | windows: never kept inside
    if (e.key !== "Tab" || e.ctrlKey || e.metaKey || e.altKey || !box) return;
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
