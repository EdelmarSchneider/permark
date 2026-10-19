<script lang="ts">
  // The document area: header, the page (or the viewer of the file's kind), the notices around it, find in page.
  // Everything about what is shown lives in the document controller; this is the frame and the keys.
  import { untrack } from "svelte";
  import { api, type WorkspaceView } from "../../api";
  import { apiErrorText } from "../errors";
  import { conditions } from "../conditions/context.svelte";
  import { revealInTree } from "../explorer/current";
  import { t, tn } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";
  import type { WindowStore } from "../window.svelte";
  import BodyNotice from "./BodyNotice.svelte";
  import DocHeader from "./DocHeader.svelte";
  import { documents } from "./document.svelte";
  import FindBar from "./FindBar.svelte";
  import ImageZoom from "./ImageZoom.svelte";
  import MarkdownBody from "./MarkdownBody.svelte";
  import ModeSwitch from "../editing/ModeSwitch.svelte";
  import { chooseMode } from "../editing/choose";
  import { editing } from "../editing/session.svelte";
  import { ui } from "../shell/ui.svelte";

  type Props = {
    workspace: Extract<WorkspaceView, { kind: "project" | "singleFile" | "notes" }>;
    document: string | null;
    store: WindowStore;
  };
  let { workspace, document: shown, store }: Props = $props();

  let scroller: HTMLElement | undefined = $state();

  // a single file shows its own file; a project shows the document the window was told to
  const path = $derived(workspace.kind === "singleFile" ? workspace.name : shown);

  /** What the header shows as the place: a loose file of the Notes shows its name, not its internal path. */
  function headerPath(place: string): string {
    return workspace.kind === "notes" && place.startsWith("@file/")
      ? (place.split("/").at(-1) ?? place)
      : place;
  }

  const doc = $derived(documents.state);
  const view = $derived(documents.view);
  const deleted = $derived(doc.status === "ready" ? doc.deleted : null);
  const viewers = $derived(contributions.visible("viewer", conditions.current));
  const bars = $derived(
    [...contributions.visible("docBar", conditions.current)].sort((a, b) => a.order - b.order),
  );
  const viewer = $derived(view === null ? undefined : viewers.find((v) => v.id === view.format));
  const outline = $derived(view?.body.kind === "markdown" ? view.body.outline : []);
  const markdown = $derived(view?.format === "markdown");
  const readOnly = $derived(
    view === null
      ? "noViewer"
      : view.editable.editable === "readOnly"
        ? view.editable.reason
        : null,
  );
  const editingNow = $derived(editing.mode !== "view" && editing.path === documents.path);

  $effect(() => {
    documents.bind({
      note: (text) => store.notifyNote(text),
      fail: (error) => store.notifyFileOpFailed(error),
      reveal: (folder) => revealInTree(folder),
    });
    editing.bind({
      note: (text) => store.notifyNote(text),
      fail: (error) => store.notifyFileOpFailed(error),
      saved: () => documents.reload(),
      open: (next) => documents.navigate(next, null),
      readingLine: () => documents.topLine(),
      showReadingLine: (line) => {
        // the page shows again first
        requestAnimationFrame(() => requestAnimationFrame(() => documents.scrollToLine(line)));
      },
      savedText: (count, first) =>
        tn("edit.savedBlocks", count) + (first ? ` ${t("edit.firstSaveHint")}` : ""),
    });
  });

  // which workspace and document: the controller follows (idempotent: same ids do nothing)
  $effect(() => {
    documents.setWorkspace(workspace);
    // another document came without the leave guard (it saved or discarded first when it was asked)
    const shown = path;
    // only the workspace and the path decide when this runs: a failed open must not make it run again by itself
    untrack(() => {
      if (editing.path !== null && editing.path !== shown) editing.release();
      void documents.show(path);
    });
  });

  // the area goes away with the workspace (the window shows the welcome again): the controller lets go of the document
  $effect(() => () => {
    editing.release();
    documents.release();
    ui.closeFind();
  });

  // the file changed on disk, or went away
  $effect(() => {
    let off: (() => void) | null = null;
    let gone = false;
    void api
      .on("document-changed", (event) => {
        if (event.window !== store.label) return;
        editing.onDocumentChanged(event);
        documents.onChanged(event);
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

  // Escape leaves an editor for View (not while a field, a menu or a dialog has the key). The other keys are commands
  // (`core/commands`): find, back | forward, zoom, modes, save.
  $effect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.isComposing || event.defaultPrevented) return;
      if (event.key !== "Escape" || !editingNow || event.ctrlKey || event.metaKey || event.altKey)
        return;
      const inside =
        event.target instanceof Element
          ? event.target.closest("input, select, textarea, form, [role=dialog], [role=listbox]")
          : null;
      if (inside === null) {
        event.preventDefault();
        chooseMode("view");
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

{#snippet modes()}
  <ModeSwitch session={editing} {readOnly} {markdown} choose={chooseMode} />
{/snippet}

<section class="area" aria-label={t("doc.region")}>
  {#if doc.status !== "empty" && documents.path !== null}
    <DocHeader
      path={headerPath(view?.path ?? documents.path)}
      savedMs={view?.mtimeMs ?? null}
      role={view?.role ?? null}
      template={store.referenceTemplate}
      hasOutline={outline.length > 0}
      onFind={() => ui.openFind()}
      canOpenSystem={workspace.kind === "project"}
      modes={view !== null && view.body.kind !== "media" ? modes : undefined}
    />
    {#each bars as bar (bar.owner + bar.id)}
      {#await bar.view() then loaded}
        {@const Bar = loaded.default}
        <Bar />
      {/await}
    {/each}
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

        {#if editingNow && scroller}
          <!-- the editors load on first use: reading never pays for them -->
          {#await import("../editing/EditorPane.svelte") then loaded}
            {@const EditorPane = loaded.default}
            <EditorPane
              session={editing}
              {scroller}
              project={workspace.kind === "project"}
              note={(text) => store.notifyNote(text)}
            />
          {/await}
        {/if}
        {#if view.body.kind === "markdown" && scroller}
          <div class="reading" hidden={editingNow}>
            <MarkdownBody
              {view}
              {scroller}
              template={store.referenceTemplate}
              deleted={deleted !== null}
            />
          </div>
        {:else if view.body.kind === "notice"}
          <BodyNotice
            notice={view.body.notice}
            name={view.name}
            path={view.path}
            canOpenSystem={workspace.kind === "project"}
          />
        {:else if editingNow}
          <!-- the editor shows instead -->
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

    {#if ui.finding && scroller && !editingNow}
      <FindBar root={scroller} close={() => ui.closeFind()} />
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
