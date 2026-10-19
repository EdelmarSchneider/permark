<script lang="ts">
  // "Where should this folder open?" This window | New window; Cancel | Escape | backdrop = neither.
  import { t } from "../i18n";

  type Props = { answer: (choice: "here" | "new" | null) => void };
  let { answer }: Props = $props();

  let dialog: HTMLElement | undefined = $state();
  let here: HTMLButtonElement | undefined = $state();

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      answer(null);
      return;
    }
    if (e.key !== "Tab" || !dialog) return;
    // focus stays inside the dialog
    const focusable = Array.from(dialog.querySelectorAll<HTMLElement>("button:not(:disabled)"));
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

  // focus the default answer once shown; give focus back when closed
  $effect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    here?.focus();
    return () => before?.focus();
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) answer(null);
  }}
>
  <div
    bind:this={dialog}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="window-choice-title"
    tabindex="-1"
    {onkeydown}
  >
    <h2 id="window-choice-title">{t("dialog.windowTitle")}</h2>
    <p>{t("dialog.windowBody")}</p>
    <footer>
      <button type="button" class="btn" onclick={() => answer(null)}>{t("common.cancel")}</button>
      <button type="button" class="btn" onclick={() => answer("new")}
        >{t("dialog.windowNew")}</button
      >
      <button
        bind:this={here}
        type="button"
        class="btn btn--primary"
        onclick={() => answer("here")}
      >
        {t("dialog.windowHere")}
      </button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: var(--sp-5);
    background: var(--backdrop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .dialog {
    width: min(440px, 100%);
    padding: var(--sp-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  h2 {
    margin: 0 0 var(--sp-2);
    font-size: 15px;
    font-weight: 600;
  }
  p {
    margin: 0 0 var(--sp-5);
    color: var(--muted);
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
</style>
