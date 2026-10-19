<script lang="ts">
  // The document area: header, the page (or the viewer of the file's kind), the notices around it, find in page.
  // Everything about what is shown lives in the document controller; this is the frame and the keys.
  import { api, type WorkspaceView } from "../../api";
  import { apiErrorText } from "../errors";
  import { conditions } from "../conditions/context.svelte";
  import { revealInTree } from "../explorer/current";
  import { t } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";
  import type { WindowStore } from "../window.svelte";
  import BodyNotice from "./BodyNotice.svelte";
  import DocHeader from "./DocHeader.svelte";
  import { documents } from "./document.svelte";
  import FindBar from "./FindBar.svelte";
  import ImageZoom from "./ImageZoom.svelte";
  import MarkdownBody from "./MarkdownBody.svelte";
  import { reading } from "./reading.svelte";

  type Props = {
    workspace: Extract<WorkspaceView, { kind: "project" | "singleFile" }>;
    document: string | null;
    store: WindowStore;
  };
  let { workspace, document: shown, store }: Props = $props();

  let scroller: HTMLElement | undefined = $state();
  let finding = $state(false);

  // a single file shows its own file; a project shows the document the window was told to
  const path = $derived(workspace.kind === "singleFile" ? workspace.name : shown);

  const doc = $derived(documents.state);
  const view = $derived(documents.view);
  const deleted = $derived(doc.status === "ready" ? doc.deleted : null);
  const viewers = $derived(contributions.visible("viewer", conditions.current));
  const viewer = $derived(view === null ? undefined : viewers.find((v) => v.id === view.format));
  const outline = $derived(view?.body.kind === "markdown" ? view.body.outline : []);

  $effect(() => {
    documents.bind({
      note: (text) => store.notifyNote(text),
      fail: (error) => store.notifyFileOpFailed(error),
      reveal: (folder) => revealInTree(folder),
    });
  });

  // which workspace and document: the controller follows (idempotent: same ids do nothing)
  $effect(() => {
    documents.setWorkspace(workspace);
    void documents.show(path);
  });

  // the area goes away with the workspace (the window shows the welcome again): the controller lets go of the document
  $effect(() => () => documents.release());

  // the file changed on disk, or went away
  $effect(() => {
    let off: (() => void) | null = null;
    let gone = false;
    void api
      .on("document-changed", (event) => {
        if (event.window === store.label) documents.onChanged(event);
      })
      .then((stop) => {
        if (gone) stop();
        else off = stop;
      });
    return () => {
      gone = true;
      off?.();
    };
  });

  // keys: find, back | forward, zoom (IME and dead keys never fire them)
  $effect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.isComposing || event.defaultPrevented) return;
      const mod = event.ctrlKey || event.metaKey;
      if (mod && !event.altKey && event.key.toLowerCase() === "f") {
        event.preventDefault();
        finding = true;
      } else if (event.altKey && !mod && event.key === "ArrowLeft") {
        event.preventDefault();
        void documents.back();
      } else if (event.altKey && !mod && event.key === "ArrowRight") {
        event.preventDefault();
        void documents.forward();
      } else if (mod && !event.altKey && (event.key === "=" || event.key === "+")) {
        event.preventDefault();
        reading.zoomBy(1);
      } else if (mod && !event.altKey && (event.key === "-" || event.key === "_")) {
        event.preventDefault();
        reading.zoomBy(-1);
      } else if (mod && !event.altKey && event.key === "0") {
        event.preventDefault();
        reading.setZoom(100);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const deletedAt = $derived(
    deleted === null
      ? ""
      : new Date(deleted.at).toLocaleTimeString(undefined, { timeStyle: "short" }),
  );
</script>

<section class="area" aria-label={t("doc.region")}>
  {#if doc.status !== "empty" && documents.path !== null}
    <DocHeader
      path={view?.path ?? documents.path}
      savedMs={view?.mtimeMs ?? null}
      role={view?.role ?? null}
      template={store.referenceTemplate}
      hasOutline={outline.length > 0}
      onFind={() => (finding = true)}
      canOpenSystem={workspace.kind === "project"}
    />
  {/if}

  <div class="pane">
    <div class="scroller" data-scroller bind:this={scroller}>
      {#if doc.status === "loading"}
        <div class="loading" role="progressbar" aria-label={t("doc.loading")}></div>
      {/if}

      {#if doc.status === "failed"}
        <div class="message" role="alert">
          <p class="title">{t("doc.error.title")}</p>
          <p class="text">{apiErrorText(doc.error)}</p>
        </div>
      {:else if view !== null}
        {#if deleted}
          <div class="banner" role="status">
            <span>{t("doc.deleted.title", { time: deletedAt })} {t("doc.deleted.text")}</span>
            {#if deleted.movedTo}
              {@const moved = deleted.movedTo}
              <button
                type="button"
                class="btn btn--sm"
                onclick={() => documents.followMoved(moved)}
              >
                {t("doc.deleted.moved", { path: moved })}
              </button>
            {/if}
          </div>
        {/if}
        {#if view.notes.length > 0}
          <p class="notes">
            {#each view.notes as note, at (note)}{at > 0 ? " · " : ""}{t(
                note === "utf16"
                  ? "note.utf16"
                  : note === "lossy"
                    ? "note.lossy"
                    : "note.deepNesting",
              )}{/each}
          </p>
        {/if}

        {#if view.body.kind === "markdown" && scroller}
          <MarkdownBody
            {view}
            {scroller}
            template={store.referenceTemplate}
            deleted={deleted !== null}
          />
        {:else if view.body.kind === "notice"}
          <BodyNotice
            notice={view.body.notice}
            name={view.name}
            path={view.path}
            canOpenSystem={workspace.kind === "project"}
          />
        {:else if view.body.kind === "text" || view.body.kind === "media"}
          {#if viewer}
            {#key view.path}
              {#await viewer.view() then loaded}
                {@const Viewer = loaded.default}
                <Viewer {view} workspace={workspace.id} />
              {/await}
            {/key}
          {:else}
            <p class="notes">{t("doc.loading")}</p>
          {/if}
        {/if}
      {:else if doc.status === "empty"}
        <div class="message">
          <p class="title">{t("doc.none.title")}</p>
          <p class="text">{t("doc.none.text")}</p>
        </div>
      {/if}
    </div>

    {#if finding && scroller}
      <FindBar root={scroller} close={() => (finding = false)} />
    {/if}
  </div>
</section>

{#if documents.zoomed}
  <ImageZoom
    src={documents.zoomed.src}
    alt={documents.zoomed.alt}
    close={() => (documents.zoomed = null)}
  />
{/if}

<style>
  .area {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .pane {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .scroller {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    overscroll-behavior: contain;
  }
  .message {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-8) var(--sp-5);
    text-align: center;
  }
  .message p,
  .notes {
    margin: 0;
    max-width: var(--measure);
  }
  .title {
    font: 600 var(--fs-reading) / var(--lh-reading) var(--font-reading);
  }
  .text {
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .notes {
    margin: var(--sp-2) auto 0;
    padding: 0 var(--sp-5);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .banner {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-wrap: wrap;
    gap: var(--sp-3);
    padding: var(--sp-2) var(--sp-4);
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--git-deleted) 8%, var(--bg));
    color: var(--git-deleted-text);
    font-size: var(--fs-ui);
  }
  /* a thin bar that only shows when opening takes more than a moment */
  .loading {
    position: sticky;
    top: 0;
    z-index: 5;
    height: 2px;
    background: var(--accent);
    transform-origin: left;
    opacity: 0;
    animation: pm-loading 1.2s var(--ease) 150ms infinite;
  }
  @keyframes pm-loading {
    0% {
      opacity: 1;
      transform: scaleX(0.05);
    }
    80% {
      opacity: 1;
      transform: scaleX(0.9);
    }
    100% {
      opacity: 0;
      transform: scaleX(1);
    }
  }
</style>
