<script lang="ts">
  import { onDestroy } from "svelte";
  import { t } from "../i18n";
  import { READING_WIDTHS } from "../readingWidth";
  import { DEFAULT_REF_TEMPLATE, formatReference } from "../reference";
  import { docs } from "../stores/docs.svelte";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import { updates } from "../stores/updates.svelte";
  import { describeUpdate, isBusy } from "../updates";
  import IconButton from "./IconButton.svelte";
  import Popover, { type Anchor } from "./Popover.svelte";

  const SAMPLE = "docs/specs/live-feed.md";

  // anchor next to whichever settings button is on screen (activity bar, or the header in Simple mode)
  function findAnchor(): Anchor {
    const button = document.querySelector<HTMLElement>("[data-settings-anchor]");
    if (button) return button.getBoundingClientRect();
    return { left: window.innerWidth - 40, right: window.innerWidth - 8, top: 8, bottom: 40 };
  }
  const anchor = findAnchor();
  const simple = anchor.left > window.innerWidth / 2;

  const themes = ["system", "light", "dark"] as const;
  const preview = $derived(formatReference(session.refTemplate, docs.path ?? SAMPLE));
  const update = $derived(updates.state);
  // the button already says "Checking..."
  const status = $derived(update.phase === "checking" ? "" : describeUpdate(update));

  // "You're up to date" is the answer to the last check; it is not kept for the next time the popover opens
  onDestroy(() => updates.dismissResult());
</script>

<Popover
  {anchor}
  variant="panel"
  side={simple ? "bottom" : "right"}
  align={simple ? "end" : "start"}
  label={t("settings.title")}
  onclose={() => (ui.settingsOpen = false)}
>
  <div class="field">
    <span class="field__label">{t("settings.theme")}</span>
    <div class="segmented" role="group" aria-label={t("settings.theme")}>
      {#each themes as theme (theme)}
        <button
          type="button"
          aria-pressed={session.theme === theme}
          onclick={() => session.setTheme(theme)}
        >
          {t(`theme.${theme}`)}
        </button>
      {/each}
    </div>
  </div>
  <div class="field">
    <span class="field__label">{t("settings.width")}</span>
    <div class="segmented" role="group" aria-label={t("settings.width")}>
      {#each READING_WIDTHS as width (width)}
        <button
          type="button"
          aria-pressed={session.width === width}
          onclick={() => session.setWidth(width)}
        >
          {t(`width.${width}`)}
        </button>
      {/each}
    </div>
  </div>
  <div class="field">
    <div class="field__row">
      <span class="field__label">{t("settings.zoom")}</span>
      <span class="field__row">
        <IconButton
          small
          icon="chevron-down"
          label={t("menu.zoomOut")}
          onclick={() => session.zoomBy(-1)}
        />
        <button
          type="button"
          class="btn btn--sm"
          onclick={() => session.setZoom(1)}
          title={t("status.zoomHint")}
        >
          {Math.round(session.zoom * 100)}%
        </button>
        <IconButton
          small
          icon="chevron-up"
          label={t("menu.zoomIn")}
          onclick={() => session.zoomBy(1)}
        />
      </span>
    </div>
  </div>
  <div class="field">
    <label class="field__label" for="ref-template">{t("settings.refTemplate")}</label>
    <input
      id="ref-template"
      class="text-input"
      value={session.refTemplate}
      placeholder={DEFAULT_REF_TEMPLATE}
      spellcheck="false"
      autocomplete="off"
      oninput={(e) => session.setRefTemplate(e.currentTarget.value)}
    />
    <span class="field__preview" aria-live="polite">{t("settings.refPreview", { preview })}</span>
  </div>
  <div class="field">
    <span class="field__label">{t("settings.updates")}</span>
    <div class="field__row">
      {#if update.phase === "available"}
        <button
          type="button"
          class="btn btn--sm btn--primary"
          onclick={() => void updates.install()}
        >
          {t("update.restart")}
        </button>
      {:else}
        <button
          type="button"
          class="btn btn--sm"
          disabled={isBusy(update)}
          onclick={() => void updates.check(true)}
        >
          {update.phase === "checking" ? t("update.checking") : t("update.check")}
        </button>
      {/if}
    </div>
    <span class="field__status" role="status">{status}</span>
  </div>
</Popover>
