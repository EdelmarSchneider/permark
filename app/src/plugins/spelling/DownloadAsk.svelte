<script lang="ts">
  // The question before a dictionary is downloaded: its language, size and license. Only Download starts it; Esc |
  // Cancel | a click beside the box leave everything as it was. The keyboard starts on Download and goes back to what
  // had it before.
  import { sizeText, t } from "../../core/i18n";
  import { dialogFocus } from "../../core/shell/dialogFocus";
  import { languageLabel } from "./names";
  import type { DownloadAsk, SpellingStore } from "./store.svelte";

  type Props = { store: SpellingStore; asked: DownloadAsk };
  let { store, asked }: Props = $props();

  const ID = "spelling-download";

  const language = $derived(store.languageOf(asked.lang));
  /** An SPDX expression reads without its outer parentheses ("MIT AND BSD"). */
  const license = $derived(language?.license.replace(/^\((.*)\)$/, "$1") ?? "");
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) store.closeAsk();
  }}
>
  <div
    class="box"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{ID}-title"
    aria-describedby="{ID}-note"
    tabindex="-1"
    use:dialogFocus={{ initial: "[data-initial]", onEscape: () => store.closeAsk() }}
  >
    <h2 id="{ID}-title">
      {t("spelling.download.question", {
        language: languageLabel(asked.lang),
        size: sizeText(language?.bytes ?? 0),
        license,
      })}
    </h2>
    <p id="{ID}-note">{t("spelling.download.note")}</p>
    <footer>
      <button type="button" class="btn btn--sm" onclick={() => store.closeAsk()}>
        {t("common.cancel")}
      </button>
      <button
        type="button"
        class="btn btn--sm btn--primary"
        data-initial
        onclick={() => void store.confirmDownload()}
      >
        {t("spelling.download.confirm")}
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
  .box {
    width: min(440px, 100%);
    padding: var(--sp-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    color: var(--text);
    box-shadow: var(--shadow-pop);
    font-size: var(--fs-ui);
    outline: none;
  }
  h2 {
    margin: 0 0 var(--sp-2);
    font-size: 15px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  p {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
    margin-top: var(--sp-4);
  }
</style>
