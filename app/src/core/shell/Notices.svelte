<script lang="ts">
  // Quiet notices (bottom right): failed opens, state set aside, settings not kept. Dismissable; never block.
  import { apiErrorText, ipcErrorText } from "../errors";
  import { t } from "../i18n";
  import type { Notice, WindowStore } from "../window.svelte";

  type Props = { store: WindowStore };
  let { store }: Props = $props();

  function text(notice: Notice): string {
    switch (notice.kind) {
      case "openFailed":
        return t("welcome.openFailed", { path: notice.path, message: ipcErrorText(notice.error) });
      case "apiFailed":
        return notice.path === null
          ? apiErrorText(notice.error)
          : t("welcome.openFailed", { path: notice.path, message: apiErrorText(notice.error) });
      case "dropOne":
        return t("welcome.dropOne");
      case "stateRecovered":
        return t("notice.stateRecovered", { file: notice.notice.file });
      case "settingNotSaved":
        return t("settings.notSaved", { message: apiErrorText(notice.error) });
      case "fileOpFailed":
        return apiErrorText(notice.error);
      case "note":
      case "action":
        return notice.text;
      default:
        return notice satisfies never;
    }
  }
</script>

{#if store.notices.length > 0}
  <div class="notices" role="status" aria-live="polite">
    {#each store.notices as notice (notice.id)}
      <div class="notice">
        <span class="text">{text(notice)}</span>
        {#if notice.kind === "action"}
          {#each notice.actions as action (action.label)}
            <button
              type="button"
              class="btn btn--sm"
              onclick={() => {
                store.dismiss(notice.id);
                action.run();
              }}
            >
              {action.label}
            </button>
          {/each}
        {/if}
        <button
          type="button"
          class="btn btn--ghost btn--sm"
          onclick={() => store.dismiss(notice.id)}
        >
          {t("common.dismiss")}
        </button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .notices {
    position: fixed;
    right: var(--sp-4);
    bottom: calc(var(--h-status) + var(--sp-3));
    z-index: 45;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: min(420px, calc(100vw - 2 * var(--sp-4)));
  }
  .notice {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-2) var(--sp-2) var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
</style>
