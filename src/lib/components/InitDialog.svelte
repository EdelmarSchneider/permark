<script lang="ts">
  import { onMount } from "svelte";
  import { errorMessage } from "../errors";
  import { t } from "../i18n";
  import { registerOverlay, rememberFocus } from "../overlays";
  import { app } from "../stores/app.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";

  let busy = $state(false);
  let error = $state<string | null>(null);
  let confirm: HTMLButtonElement | undefined = $state();
  let dialog: HTMLElement | undefined = $state();

  function close(): void {
    if (!busy) ui.initDialog = false;
  }

  async function initialize(): Promise<void> {
    busy = true;
    error = null;
    try {
      await app.initProject();
      ui.initDialog = false;
    } catch (e) {
      error = t("init.failed", { message: errorMessage(e) });
    } finally {
      busy = false;
    }
  }

  // keep Tab inside the dialog
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
    confirm?.focus();
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
    aria-labelledby="init-title"
    tabindex="-1"
    {onkeydown}
  >
    <h2 id="init-title">{t("init.title")}</h2>
    <p>{t("init.intro", { name: vault.info?.name ?? "" })}</p>
    <dl>
      <dt>config.toml</dt>
      <dd>{t("init.config")}</dd>
      <dt>cache/</dt>
      <dd>{t("init.cache")}</dd>
      <dt>local/</dt>
      <dd>{t("init.local")}</dd>
    </dl>
    <p style="margin:0">{t("init.note")}</p>
    {#if error}<p class="inline-error" role="alert">{error}</p>{/if}
    <footer>
      <button type="button" class="btn" disabled={busy} onclick={close}>{t("common.cancel")}</button
      >
      <button
        bind:this={confirm}
        type="button"
        class="btn btn--primary"
        disabled={busy}
        onclick={() => void initialize()}
      >
        {#if busy}<span class="spinner"></span>{/if}{t("init.confirm")}
      </button>
    </footer>
  </div>
</div>
