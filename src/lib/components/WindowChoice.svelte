<script lang="ts">
  // Where a folder opens: this window, or a new one. Dismissing chooses neither.
  import { onMount } from "svelte";
  import { t } from "../i18n";
  import { registerOverlay, rememberFocus } from "../overlays";
  import { app } from "../stores/app.svelte";

  let dialog: HTMLElement | undefined = $state();
  let here: HTMLButtonElement | undefined = $state();

  function close(): void {
    app.answerWindow(null);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key !== "Tab" || !dialog) return;
    const focusable = Array.from(dialog.querySelectorAll<HTMLElement>("button:not(:disabled)"));
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  onMount(() => {
    const restore = rememberFocus();
    here?.focus();
    const unregister = registerOverlay(close);
    return () => {
      unregister();
      restore();
    };
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
    role="dialog"
    aria-modal="true"
    aria-labelledby="window-choice-title"
    tabindex="-1"
    {onkeydown}
  >
    <h2 id="window-choice-title">{t("dialog.windowTitle")}</h2>
    <p>{t("dialog.windowBody")}</p>
    <footer>
      <button type="button" class="btn" onclick={close}>{t("common.cancel")}</button>
      <button type="button" class="btn" onclick={() => app.answerWindow("new")}
        >{t("dialog.windowNew")}</button
      >
      <button
        bind:this={here}
        type="button"
        class="btn btn--primary"
        onclick={() => app.answerWindow("here")}>{t("dialog.windowHere")}</button
      >
    </footer>
  </div>
</div>
