<script lang="ts">
  // A picture, a sound or a video shown in place, from the asset protocol (the file is never read into the page's
  // memory as text). Pictures can be fitted or shown at their real size and zoomed; a file the webview cannot show or
  // play is offered to the system's program.
  import { api } from "../../api";
  import { t } from "../../core/i18n";
  import type { ViewerProps } from "../../core/plugins/props";

  let { view, workspace }: ViewerProps = $props();

  const body = $derived(view.body.kind === "media" ? view.body : null);
  const src = $derived(api.assetUrl(workspace, view.path, String(view.mtimeMs)));

  type Fit = "fit" | "actual";
  let fit = $state<Fit>("fit");
  let zoom = $state(1);
  let failed = $state(false);
  let natural = $state<{ width: number; height: number } | null>(null);

  function onload(event: Event): void {
    const img = event.currentTarget;
    if (img instanceof HTMLImageElement)
      natural = { width: img.naturalWidth, height: img.naturalHeight };
  }

  function step(direction: 1 | -1): void {
    fit = "actual";
    zoom = Math.min(8, Math.max(0.1, zoom * (direction === 1 ? 1.25 : 0.8)));
  }
</script>

<div class="media">
  {#if failed}
    <div class="failed" role="alert">
      <p class="title">
        {body?.media === "image" ? t("media.imageFailed.title") : t("media.cantPlay.title")}
      </p>
      <p class="text">
        {body?.media === "image" ? t("media.imageFailed.text") : t("media.cantPlay.text")}
      </p>
      <button
        type="button"
        class="btn"
        onclick={() => void api.openExternal(view.path, "defaultApp")}
      >
        {t("menu.openDefaultApp")}
      </button>
    </div>
  {:else if body?.media === "image"}
    <div class="bar" role="toolbar" aria-label={t("media.zoomBar")}>
      <button
        type="button"
        class="btn btn--sm"
        aria-pressed={fit === "fit"}
        onclick={() => ((fit = "fit"), (zoom = 1))}
        title={t("media.fitHint")}
      >
        {t("media.fit")}
      </button>
      <button
        type="button"
        class="btn btn--sm"
        aria-pressed={fit === "actual" && zoom === 1}
        onclick={() => ((fit = "actual"), (zoom = 1))}
      >
        {t("media.actualSize")}
      </button>
      <button
        type="button"
        class="btn btn--sm"
        aria-label={t("media.zoomOut")}
        title={t("media.zoomOutHint")}
        onclick={() => step(-1)}>−</button
      >
      <button
        type="button"
        class="btn btn--sm"
        aria-label={t("media.zoomIn")}
        title={t("media.zoomInHint")}
        onclick={() => step(1)}>+</button
      >
      {#if natural}
        <span class="dims"
          >{t("media.dimensions", { width: natural.width, height: natural.height })}</span
        >
      {/if}
    </div>
    <div class="stage" class:is-fit={fit === "fit"}>
      <img
        {src}
        alt={view.name}
        class:is-fit={fit === "fit"}
        style:width={fit === "actual" && natural ? `${natural.width * zoom}px` : undefined}
        {onload}
        onerror={() => (failed = true)}
        draggable="false"
      />
    </div>
  {:else if body?.media === "audio"}
    <div class="player">
      <audio {src} controls preload="metadata" onerror={() => (failed = true)}></audio>
    </div>
  {:else if body?.media === "video"}
    <div class="player">
      <!-- svelte-ignore a11y_media_has_caption -->
      <video {src} controls preload="metadata" onerror={() => (failed = true)}></video>
    </div>
  {/if}
</div>

<style>
  .media {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding: var(--sp-3) var(--sp-5) var(--sp-8);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
  .dims {
    margin-left: auto;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-variant-numeric: tabular-nums;
  }
  .stage {
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    /* behind a picture with transparency */
    background: conic-gradient(
        var(--checker-b) 25%,
        var(--checker-a) 0 50%,
        var(--checker-b) 0 75%,
        var(--checker-a) 0
      )
      0 0 / 16px 16px;
    display: grid;
    place-items: center;
    min-height: 200px;
  }
  .stage.is-fit {
    overflow: hidden;
  }
  img {
    display: block;
    max-width: none;
  }
  img.is-fit {
    max-width: 100%;
    max-height: calc(100vh - 220px);
    object-fit: contain;
  }
  .player {
    display: grid;
    place-items: center;
  }
  video {
    max-width: 100%;
    max-height: calc(100vh - 200px);
    background: var(--video-bg);
    border-radius: var(--radius-card);
  }
  audio {
    width: min(100%, 520px);
  }
  .failed {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-8) var(--sp-5);
    text-align: center;
  }
  .failed p {
    margin: 0;
    max-width: var(--measure);
  }
  .title {
    font: 600 var(--fs-reading) / var(--lh-reading) var(--font-reading);
  }
  .text {
    color: var(--muted);
  }
  button[aria-pressed="true"] {
    background: var(--accent-soft);
    color: var(--accent-text);
  }
</style>
