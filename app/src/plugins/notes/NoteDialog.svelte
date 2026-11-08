<script lang="ts">
  // The frame of the Notes dialogs: backdrop, title, body, footer. Escape and the backdrop close it; focus stays in it.
  import type { Snippet } from "svelte";
  import { dialogFocus } from "../../core/shell/dialogFocus";

  type Props = {
    title: string;
    close: () => void;
    wide?: boolean;
    children: Snippet;
    footer: Snippet;
  };
  let { title, close, wide = false, children, footer }: Props = $props();

  const id = `notes-dialog-${Math.random().toString(36).slice(2, 8)}`;
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) close();
  }}
>
  <!-- focus enters on the first field | button: `data-first` wins, `data-skip` is passed over -->
  <div
    class="dialog"
    class:wide
    role="dialog"
    aria-modal="true"
    aria-labelledby={id}
    tabindex="-1"
    use:dialogFocus={{ initial: "[data-first], input, button:not([data-skip])", onEscape: close }}
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
