<script lang="ts">
  // Follow mode in the status bar: that it is on (and how to stop it), that it waits while the person edits, and the
  // way back from the last jump.
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Radio from "@lucide/svelte/icons/radio";
  import { t } from "../../core/i18n";
  import { follow } from "./store.svelte";

  const from = $derived(follow.jumpedFrom?.split("/").at(-1) ?? null);
</script>

{#if follow.on}
  <span class="follow" role="status">
    <Radio size={13} aria-hidden="true" />
    {t("follow.on")}
    {#if follow.paused}<span class="paused">· {t("follow.paused")}</span>{/if}
    {#if from}
      <button type="button" class="link" title={t("follow.back")} onclick={() => follow.goBack()}>
        <ArrowLeft size={12} aria-hidden="true" />
        {t("follow.jumped", { name: from })}
      </button>
    {/if}
    <button type="button" class="link" onclick={() => follow.stop()}>{t("follow.stop")}</button>
  </span>
{/if}

<style>
  .follow {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 6px;
    border-radius: 999px;
    /* the accent as small text on its soft tint: AA in every theme and tone (--accent itself is below it here) */
    background: var(--accent-soft);
    color: color-mix(in srgb, var(--accent-text) 85%, var(--text));
    font-weight: 600;
  }
  /* quieter by weight, not by color: the pill's text color is the one that keeps AA on its tint */
  .paused {
    font-weight: 400;
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 0 4px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: inherit;
    font: inherit;
    font-weight: 400;
    text-decoration: underline;
    cursor: pointer;
  }
  .link:hover {
    background: var(--hover);
  }
</style>
