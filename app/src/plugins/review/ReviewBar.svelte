<script lang="ts">
  // The strip under the header: "Changed since you last read · 3 changes [Review]", and in review the tools of it:
  // previous | next, Keep all, Revert all, Undo, what to compare with, Done.
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import type { Basis } from "../../api";
  import { t, tn } from "../../core/i18n";
  import { review } from "./store.svelte";

  const state = $derived(review.state);
  const BASES: { value: Basis; label: "review.basis.read" | "review.basis.commit" }[] = [
    { value: "read", label: "review.basis.read" },
    { value: "commit", label: "review.basis.commit" },
  ];
</script>

{#if state.status === "offer"}
  <div class="bar" role="region" aria-label={t("review.bar")}>
    <span>{tn("review.offer", state.count)}</span>
    <button type="button" class="btn btn--sm" onclick={() => void review.begin()}>
      {t("review.review")}
    </button>
  </div>
{:else if state.status === "reviewing"}
  {@const view = state.view}
  <div class="bar reviewing" role="region" aria-label={t("review.bar")}>
    <span class="live" aria-live="polite">{review.announcement}</span>
    <strong>{tn("review.reviewing", view.count)}</strong>
    {#if view.count > 0}
      <span class="position">{t("review.position", { i: state.at + 1, n: view.count })}</span>
      <button
        type="button"
        class="icon"
        title={t("review.prev")}
        aria-label={t("review.prev")}
        onclick={() => review.go(-1)}><ChevronUp size={15} /></button
      >
      <button
        type="button"
        class="icon"
        title={t("review.next")}
        aria-label={t("review.next")}
        onclick={() => review.go(1)}><ChevronDown size={15} /></button
      >
    {/if}
    <span class="spacer"></span>
    <span class="segments" role="radiogroup" aria-label={t("review.basis.label")}>
      {#each BASES as option (option.value)}
        <button
          type="button"
          role="radio"
          aria-checked={review.basis === option.value}
          class={["segment", review.basis === option.value && "is-on"]}
          onclick={() => void review.setBasis(option.value)}>{t(option.label)}</button
        >
      {/each}
    </span>
    {#if view.canKeep}
      <button
        type="button"
        class="btn btn--sm"
        title={t("review.keepAllHint")}
        onclick={() => void review.keepAll()}>{t("review.keepAll")}</button
      >
    {/if}
    <button
      type="button"
      class="btn btn--sm"
      title={t("review.revertAllHint")}
      onclick={() => void review.revertAll()}>{t("review.revertAll")}</button
    >
    <button
      type="button"
      class="btn btn--sm"
      disabled={review.undoable === 0}
      onclick={() => void review.undo()}
    >
      <Undo2 size={13} aria-hidden="true" />
      {t("review.undo")}
    </button>
    <button
      type="button"
      class="btn btn--sm"
      title={t("review.doneHint")}
      onclick={() => void review.done()}>{t("review.done")}</button
    >
  </div>
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-4);
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--accent) 7%, var(--bg));
    font-size: var(--fs-ui-sm);
  }
  .spacer {
    flex: 1;
  }
  .position {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .live {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .icon {
    display: inline-grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--hover);
    color: var(--text);
  }
  .segments {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  .segment {
    padding: 2px 8px;
    border: 0;
    border-radius: calc(var(--radius-control) - 2px);
    background: transparent;
    color: var(--muted);
    font: inherit;
    cursor: pointer;
  }
  .segment.is-on {
    background: var(--bg);
    color: var(--text);
    font-weight: 600;
  }
</style>
