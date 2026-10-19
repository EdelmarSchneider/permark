<script lang="ts">
  // The reading look of this project: width and zoom. A small popover under its button; Escape or a click elsewhere
  // closes it.
  import { t } from "../i18n";
  import { WIDTHS, ZOOM_DEFAULT, ZOOM_MAX, ZOOM_MIN, reading, type Width } from "./reading.svelte";

  type Props = { close: () => void };
  let { close }: Props = $props();

  let panel: HTMLElement | undefined = $state();

  const label: Record<Width, "width.comfortable" | "width.wide" | "width.full"> = {
    comfortable: "width.comfortable",
    wide: "width.wide",
    full: "width.full",
  };

  // a click outside the popover (or its button) closes it
  $effect(() => {
    const onOutside = (event: Event): void => {
      const target = event.target;
      if (!(target instanceof Node) || panel?.contains(target)) return;
      if (target instanceof Element && target.closest("[data-reading-trigger]")) return;
      close();
    };
    window.addEventListener("pointerdown", onOutside, true);
    // focus moving elsewhere (find in page, say) closes it too
    window.addEventListener("focusin", onOutside);
    return () => {
      window.removeEventListener("pointerdown", onOutside, true);
      window.removeEventListener("focusin", onOutside);
    };
  });

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="reading-menu"
  bind:this={panel}
  role="dialog"
  aria-label={t("reading.menu")}
  tabindex="-1"
  {onkeydown}
>
  <div class="menu-label">{t("settings.width")}</div>
  <div class="segments" role="radiogroup" aria-label={t("settings.width")}>
    {#each WIDTHS as width (width)}
      <button
        type="button"
        class="segment"
        role="radio"
        aria-checked={reading.width === width}
        onclick={() => reading.setWidth(width)}
      >
        {t(label[width])}
      </button>
    {/each}
  </div>
  <div class="menu-label">{t("settings.zoom")}</div>
  <div class="zoom">
    <button
      type="button"
      class="btn btn--sm"
      title={t("media.zoomOutHint")}
      aria-label={t("media.zoomOut")}
      disabled={reading.zoom <= ZOOM_MIN}
      onclick={() => reading.zoomBy(-1)}>−</button
    >
    <output class="zoom-value" aria-live="polite">{reading.zoom}%</output>
    <button
      type="button"
      class="btn btn--sm"
      title={t("media.zoomInHint")}
      aria-label={t("media.zoomIn")}
      disabled={reading.zoom >= ZOOM_MAX}
      onclick={() => reading.zoomBy(1)}>+</button
    >
    <button
      type="button"
      class="btn btn--ghost btn--sm"
      disabled={reading.zoom === ZOOM_DEFAULT}
      onclick={() => reading.setZoom(ZOOM_DEFAULT)}
    >
      {t("cmd.zoomReset")}
    </button>
  </div>
</div>

<style>
  .reading-menu {
    position: absolute;
    top: calc(100% + 4px);
    right: var(--sp-2);
    z-index: 30;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    width: 260px;
    padding: var(--sp-2) var(--sp-3) var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .menu-label {
    padding-left: 0;
  }
  .segments {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    overflow: hidden;
  }
  .segment {
    height: 26px;
    padding: 0 6px;
    border: 0;
    border-right: 1px solid var(--border);
    background: var(--bg);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .segment:last-child {
    border-right: 0;
  }
  .segment:hover {
    color: var(--text);
    background: var(--hover);
  }
  .segment[aria-checked="true"] {
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 600;
  }
  .zoom {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
  .zoom-value {
    min-width: 3.2em;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }
</style>
