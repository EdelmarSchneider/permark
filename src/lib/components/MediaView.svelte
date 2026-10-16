<script lang="ts">
  import { tick, untrack } from "svelte";
  import { drawnSize } from "../imageZoom";
  import { t } from "../i18n";
  import { basename } from "../paths";
  import { assetUrl } from "../platform";
  import { mediaSummary } from "../mediaInfo";
  import { app } from "../stores/app.svelte";
  import { media } from "../stores/media.svelte";
  import type { DocumentView } from "../types";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Notice from "./Notice.svelte";

  /** A picture, a sound or a video: `kind` says which. The file itself is fetched through the asset scheme. */
  type Props = { view: DocumentView & { kind: "image" | "audio" | "video" } };
  let { view }: Props = $props();

  /** The room around the picture, in pixels; the stylesheet gives the frame the same. */
  const FRAME = 16;

  let stage: HTMLElement | undefined = $state();
  /** The webview could not decode or play the file. */
  let failed = $state(false);

  const name = $derived(basename(view.path));
  // the file's time is the version: a file that changed on disk is a new address, so it is fetched again
  const url = $derived(assetUrl(view.path, view.modifiedMs));
  const natural = $derived(media.natural);
  const drawn = $derived(natural ? drawnSize(natural, media.scale) : null);
  const summary = $derived(mediaSummary(view.kind, view.sizeBytes, media.facts).join(" · "));

  // a different file starts over; the same file read again (it changed on disk) keeps its zoom
  $effect(() => {
    const path = view.path;
    untrack(() => media.show(path));
  });
  // a changed file gets another chance
  $effect(() => {
    void url;
    untrack(() => (failed = false));
  });

  // ---- the picture -----------------------------------------------------------------------------------

  function decoded(e: Event): void {
    const img = e.currentTarget as HTMLImageElement;
    media.noteFacts(view.path, { width: img.naturalWidth, height: img.naturalHeight });
  }

  /** Keeps the middle of the picture in the middle of the pane when its size changes. */
  async function zoomed(change: () => void): Promise<void> {
    const pane = stage;
    if (!pane) return change();
    const centre = {
      x: (pane.scrollLeft + pane.clientWidth / 2) / Math.max(1, pane.scrollWidth),
      y: (pane.scrollTop + pane.clientHeight / 2) / Math.max(1, pane.scrollHeight),
    };
    change();
    await tick();
    pane.scrollLeft = centre.x * pane.scrollWidth - pane.clientWidth / 2;
    pane.scrollTop = centre.y * pane.scrollHeight - pane.clientHeight / 2;
  }

  function onwheel(e: WheelEvent): void {
    if (!e.ctrlKey) return;
    // the same zoom as in a document, here on the picture; the document view must not zoom its text as well
    e.preventDefault();
    e.stopPropagation();
    void zoomed(() => media.zoomBy(e.deltaY < 0 ? 1 : -1));
  }

  // The room the picture is fitted into follows the pane.
  $effect(() => {
    const pane = stage;
    if (!pane || typeof ResizeObserver === "undefined") return;
    const measure = (): void =>
      media.setBox({
        width: Math.max(0, pane.clientWidth - 2 * FRAME),
        height: Math.max(0, pane.clientHeight - 2 * FRAME),
      });
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(pane);
    return () => watch.disconnect();
  });

  // ---- sound and video -------------------------------------------------------------------------------

  function played(e: Event): void {
    const player = e.currentTarget as HTMLMediaElement;
    media.noteFacts(view.path, {
      duration: player.duration,
      ...(player instanceof HTMLVideoElement && player.videoWidth > 0
        ? { width: player.videoWidth, height: player.videoHeight }
        : {}),
    });
  }
</script>

<div class={["media", `media--${view.kind}`]} aria-label={name} role="group">
  {#if failed}
    <Notice
      icon={view.kind === "image" ? "image" : view.kind === "audio" ? "music" : "video"}
      title={view.kind === "image" ? t("media.imageFailed.title") : t("media.cantPlay.title")}
      text={view.kind === "image"
        ? t("media.imageFailed.text", { name })
        : t("media.cantPlay.text", { name })}
    >
      <button type="button" class="btn" onclick={() => void app.openWithDefaultApp(view.path)}>
        <Icon name="external" size="sm" />{t("media.openDefault")}
      </button>
      <button type="button" class="btn" onclick={() => void app.revealInFolder(view.path)}>
        <Icon name="folder" size="sm" />{t("menu.showInFileManager")}
      </button>
    </Notice>
  {:else if view.kind === "image"}
    <div class="media-bar" role="toolbar" aria-label={t("media.zoomBar")}>
      <IconButton
        small
        icon="zoom-out"
        label={t("media.zoomOut")}
        hint={t("media.zoomOutHint")}
        disabled={!natural}
        onclick={() => void zoomed(() => media.zoomBy(-1))}
      />
      <button
        type="button"
        class="btn btn--ghost btn--sm media-bar__scale"
        title={media.isFit ? t("media.actualSize") : t("media.fit")}
        disabled={!natural}
        onclick={() => void zoomed(() => media.toggleActual())}
      >
        {media.label}
      </button>
      <IconButton
        small
        icon="zoom-in"
        label={t("media.zoomIn")}
        hint={t("media.zoomInHint")}
        disabled={!natural}
        onclick={() => void zoomed(() => media.zoomBy(1))}
      />
      <IconButton
        small
        icon="fit"
        label={t("media.fit")}
        hint={t("media.fitHint")}
        pressed={media.isFit}
        disabled={!natural || media.isFit}
        onclick={() => void zoomed(() => media.reset())}
      />
      <span class="media-bar__info" role="status">{summary}</span>
    </div>
    <!-- the stage scrolls when the picture is larger than the pane; Ctrl+wheel zooms -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      bind:this={stage}
      class="image-stage"
      role="region"
      aria-label={name}
      tabindex="-1"
      {onwheel}
    >
      <div class="image-frame">
        <!-- the click toggles fit and 100%; the toolbar and the keys do the same for the keyboard -->
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
        <img
          class={[
            "image",
            media.isFit ? "image--fit" : "image--zoomed",
            !drawn && "image--unsized",
          ]}
          src={url}
          alt={name}
          draggable="false"
          style:width={drawn ? `${drawn.width}px` : undefined}
          style:height={drawn ? `${drawn.height}px` : undefined}
          onload={decoded}
          onerror={() => (failed = true)}
          onclick={() => void zoomed(() => media.toggleActual())}
        />
      </div>
    </div>
  {:else if view.kind === "audio"}
    <div class="player-stage">
      <div class="player-card">
        <Icon name="music" />
        <p class="player-card__name">{name}</p>
        <audio
          class="player"
          controls
          preload="metadata"
          src={url}
          aria-label={name}
          onloadedmetadata={played}
          onerror={() => (failed = true)}
        ></audio>
        <p class="player-card__info" role="status">{summary}</p>
        <button
          type="button"
          class="btn btn--ghost btn--sm"
          onclick={() => void app.openWithDefaultApp(view.path)}
        >
          <Icon name="external" size="sm" />{t("media.openDefault")}
        </button>
      </div>
    </div>
  {:else}
    <div class="player-stage">
      <!-- a file of the folder has no captions to offer: the system's application is one click away -->
      <!-- svelte-ignore a11y_media_has_caption -->
      <video
        class="video"
        controls
        preload="metadata"
        src={url}
        aria-label={name}
        onloadedmetadata={played}
        onerror={() => (failed = true)}
      ></video>
      <div class="video-bar">
        <span class="video-bar__info" role="status">{summary}</span>
        <button
          type="button"
          class="btn btn--ghost btn--sm"
          onclick={() => void app.openWithDefaultApp(view.path)}
        >
          <Icon name="external" size="sm" />{t("media.openDefault")}
        </button>
      </div>
    </div>
  {/if}
</div>
