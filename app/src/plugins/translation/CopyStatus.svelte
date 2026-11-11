<script lang="ts">
  // Status bar item of a document being translated into a new file, there only while that runs (it goes on while the
  // person reads or edits something else): where the run stands (getting the AI engine | the model ready, in the
  // words and with the share the review bar shows; then the part the model is on, "Translating 4 / 12") and Cancel,
  // named by the file. A screen reader hears each stage once, not every part.
  import Languages from "@lucide/svelte/icons/languages";
  import { t } from "../../core/i18n";
  import { percentText } from "../local-ai/words";
  import { copy as windowCopy, type CopyState, type TranslationCopy } from "./copy.svelte";

  type Props = { copy?: TranslationCopy };
  let { copy = windowCopy }: Props = $props();

  type Status = Extract<CopyState, { kind: "running" }>["status"];

  const shown = $derived(copy.state);

  /** The stage, as the status bar words it. */
  function label(status: Status): string {
    switch (status.kind) {
      case "engine":
        return `${t("localAi.test.engine")} ${percentText(status.receivedBytes, status.totalBytes)}`;
      case "loading":
        return `${t("translation.loading")} ${percentText(status.progress, 1)}`;
      case "checking":
        return t("translation.copy.progress", { i: status.part, n: status.parts });
      default:
        return status satisfies never;
    }
  }

  /** The stage, as it is said once when it starts. */
  function spoken(status: Status): string {
    switch (status.kind) {
      case "engine":
        return t("localAi.test.engine");
      case "loading":
        return t("translation.loading");
      case "checking":
        return t("translation.copy.busy");
      default:
        return status satisfies never;
    }
  }
</script>

{#if shown.kind === "running"}
  <span class="copy" role="group" aria-label={t("translation.copy.busy")}>
    <span class="sr-only" role="status" aria-live="polite">{spoken(shown.status)}</span>
    <Languages size={13} aria-hidden="true" />
    <span class="label" title={shown.file}>{label(shown.status)}</span>
    <button
      type="button"
      class="cancel"
      aria-label={t("translation.copy.cancel", { name: shown.file })}
      onclick={() => copy.cancel()}>{t("common.cancel")}</button
    >
  </span>
{/if}

<style>
  .copy {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    margin-right: var(--sp-3);
    /* small text on the status bar: the text accent (AA in every tone), not the accent itself */
    color: var(--accent-text);
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .cancel {
    flex: none;
    height: var(--h-status);
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }
  .cancel:hover {
    background: var(--hover);
  }
</style>
