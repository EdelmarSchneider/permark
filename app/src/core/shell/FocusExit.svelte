<script lang="ts">
  // The way out of focus mode: a small round button in the top right corner of the window. It is invisible (and takes
  // no clicks, so the page under the corner stays usable) until the pointer comes near, the button has the keyboard
  // focus, or the first moments after entering (so the person sees how to leave). While the editor holds unsaved
  // changes, a small dot says so (the header that said it is hidden).
  import Minimize2 from "@lucide/svelte/icons/minimize-2";
  import { keyHint } from "../commands/hint";
  import { editing } from "../editing/session.svelte";
  import { t } from "../i18n";
  import { ui } from "./ui.svelte";

  /** The corner that reveals the button (CSS px from the top right). */
  const NEAR_X = 160;
  const NEAR_Y = 64;
  /** The button shows this long after focus mode starts. */
  const FIRST_MS = 2500;

  let near = $state(false);
  let first = $state(true);
  let focused = $state(false);

  const shown = $derived(near || first || focused);
  const keys = $derived(keyHint("doc.focus"));
  const label = $derived(keys === null ? t("focus.leave") : `${t("focus.leave")} (${keys})`);

  // external I/O only: a timer and the pointer; both end with the component
  $effect(() => {
    const timer = setTimeout(() => (first = false), FIRST_MS);
    const move = (event: PointerEvent): void => {
      near = event.clientX >= window.innerWidth - NEAR_X && event.clientY <= NEAR_Y;
    };
    const away = (): void => {
      near = false;
    };
    window.addEventListener("pointermove", move, { passive: true });
    document.documentElement.addEventListener("pointerleave", away);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("pointermove", move);
      document.documentElement.removeEventListener("pointerleave", away);
    };
  });
</script>

<div class="exit" class:shown>
  {#if editing.unsaved}
    <span class="dot" role="img" aria-label={t("edit.unsavedTitle")} title={t("edit.unsavedTitle")}
    ></span>
  {/if}
  <button
    type="button"
    class="leave"
    aria-label={label}
    title={label}
    onfocus={() => (focused = true)}
    onblur={() => (focused = false)}
    onclick={() => ui.setFocus("off")}
  >
    <Minimize2 size={16} />
  </button>
</div>

<style>
  .exit {
    position: fixed;
    top: var(--sp-3);
    right: var(--sp-3);
    z-index: 40;
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    pointer-events: none;
  }
  .leave {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 50%;
    background: var(--surface);
    color: var(--muted);
    box-shadow: var(--shadow-chip);
    cursor: pointer;
    opacity: 0;
    pointer-events: none;
    transition:
      opacity var(--dur) var(--ease),
      color var(--dur-fast);
  }
  .shown .leave {
    opacity: 1;
    pointer-events: auto;
  }
  .leave:hover {
    color: var(--text);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    pointer-events: auto;
  }
  @media (prefers-reduced-motion: reduce) {
    .leave {
      transition: none;
    }
  }
</style>
