<script lang="ts">
  // The frame of the Notes dialogs: backdrop, title, body, footer. Escape and the backdrop close it; focus stays in it.
  import type { Snippet } from "svelte";

  type Props = {
    title: string;
    close: () => void;
    wide?: boolean;
    children: Snippet;
    footer: Snippet;
  };
  let { title, close, wide = false, children, footer }: Props = $props();

  let dialog: HTMLElement | undefined = $state();
  const id = `notes-dialog-${Math.random().toString(36).slice(2, 8)}`;

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
      return;
    }
    if (e.key !== "Tab" || !dialog) return;
    const focusable = Array.from(
      // the Tab order only (a radio list has one stop: its other rows are `tabindex=-1`)
      dialog.querySelectorAll<HTMLElement>(
        ":is(button:not(:disabled), input:not(:disabled), [tabindex='0']):not([tabindex='-1'])",
      ),
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

  // focus enters the dialog (the first field | button) and returns to where it was when the dialog goes
  $effect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog?.querySelector<HTMLElement>("[data-first], input, button:not([data-skip])")?.focus();
    return () => before?.focus();
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) close();
  }}
>
  <div
    bind:this={dialog}
    class="dialog"
    class:wide
    role="dialog"
    aria-modal="true"
    aria-labelledby={id}
    tabindex="-1"
    {onkeydown}
  >
    <h2 {id}>{title}</h2>
    <div class="body">{@render children()}</div>
    <footer>{@render footer()}</footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    /* one cell the size of the window (never the content's): the dialog's max-height holds, its body scrolls */
    grid-template: minmax(0, 1fr) / minmax(0, 1fr);
    place-items: center;
    padding: var(--sp-5);
    background: var(--backdrop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    width: min(460px, 100%);
    max-height: min(640px, 100%);
    padding: var(--sp-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  .dialog.wide {
    width: min(860px, 100%);
    height: min(560px, 100%);
  }
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    flex: 1;
    min-height: 0;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
</style>
