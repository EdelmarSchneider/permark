<script lang="ts">
  // The running import: a small card at the bottom left (the page stays usable), the item being worked on, a bar and
  // the counts, and a way to stop. What was finished stays.
  import type { ImportMode, ImportProgress } from "../../api";
  import { t, tn } from "../i18n";

  type Props = {
    /** The target folder as the person reads it. */
    where: string;
    mode: ImportMode;
    progress: ImportProgress | null;
    cancelling: boolean;
    stop: () => void;
  };
  let { where, mode, progress, cancelling, stop }: Props = $props();

  const counting = $derived(progress === null || progress.phase === "counting");
  /** 0..100; the bytes when they are known, else the items. */
  const percent = $derived.by(() => {
    if (progress === null || counting) return null;
    if (progress.bytesTotal > 0)
      return Math.min(100, (progress.bytesDone / progress.bytesTotal) * 100);
    return progress.itemsTotal > 0 ? (progress.itemsDone / progress.itemsTotal) * 100 : null;
  });
  const title = $derived(
    cancelling
      ? t("import.stopping")
      : counting
        ? t("import.counting")
        : mode === "copy"
          ? t("import.copying", { folder: where })
          : t("import.moving", { folder: where }),
  );
</script>

<div class="card" role="group" aria-label={t("import.progressLabel")}>
  <p class="title">{title}</p>
  {#if progress && progress.item !== "" && !counting}
    <p class="item" title={progress.item}>{progress.item}</p>
  {/if}
  <div
    class="bar"
    role="progressbar"
    aria-label={t("import.progressLabel")}
    aria-valuemin={0}
    aria-valuemax={100}
    aria-valuenow={percent === null ? undefined : Math.round(percent)}
  >
    <div class="fill" class:indeterminate={percent === null} style:width="{percent ?? 35}%"></div>
  </div>
  <div class="foot">
    <span class="count">
      {#if progress && progress.filesTotal > 0}
        {t("import.progressFiles", { done: progress.filesDone, total: progress.filesTotal })}
      {:else if progress}
        {tn("import.progressItems", progress.itemsTotal, { done: progress.itemsDone })}
      {/if}
    </span>
    <button type="button" class="btn btn--sm" disabled={cancelling} onclick={stop}>
      {t("common.cancel")}
    </button>
  </div>
</div>

<style>
  .card {
    position: fixed;
    left: var(--sp-4);
    bottom: calc(var(--h-status) + var(--sp-3));
    z-index: 45;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: min(320px, calc(100vw - 2 * var(--sp-4)));
    padding: var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    animation: pm-fade var(--dur) var(--ease);
  }
  p {
    margin: 0;
  }
  .title {
    font-weight: 600;
  }
  .item {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .bar {
    height: 4px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--hover);
  }
  .fill {
    height: 100%;
    border-radius: 2px;
    background: var(--accent);
  }
  .fill.indeterminate {
    animation: pm-import-slide 1.2s var(--ease) infinite;
  }
  @keyframes pm-import-slide {
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(300%);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .fill.indeterminate {
      animation: none;
      width: 100% !important;
      opacity: 0.4;
    }
  }
  @media (prefers-reduced-motion: no-preference) {
    .fill {
      transition: width var(--dur-fast) linear;
    }
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
  }
  .count {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-variant-numeric: tabular-nums;
  }
</style>
