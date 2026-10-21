<script lang="ts">
  // The page of the Notes window: the strips over the note (crash recovery, the state of a loose file), the note in its
  // editor, and the dialogs. The window follows the Notes workspace for as long as this page exists.
  import DocumentArea from "../../core/doc/DocumentArea.svelte";
  import { t } from "../../core/i18n";
  import { dropFlow } from "../../core/shell/dropView.svelte";
  import type { WorkspaceViewProps } from "../../core/plugins/props";
  import { windowStore } from "../../core/window.svelte";
  import CompareDialog from "./CompareDialog.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import { isLooseFile } from "./model";
  import MoveDialog from "./MoveDialog.svelte";
  import NoteBars from "./NoteBars.svelte";
  import { notes as store } from "./store.svelte";
  import VersionsDialog from "./VersionsDialog.svelte";

  let { workspace }: WorkspaceViewProps = $props();

  // the window follows the Notes: the list, the polling, the editor's journal; a dropped text file becomes a loose
  // file (pictures over the editor go into the note, folders and other files open as tabs)
  $effect(() => {
    const stop = store.attach(workspace);
    dropFlow.notes = { takes: isLooseFile, open: (paths) => store.openFiles(paths) };
    return () => {
      stop();
      dropFlow.notes = null;
    };
  });
</script>

<div class="page">
  <NoteBars {store} />
  {#if store.current !== null}
    <DocumentArea {workspace} document={store.current} store={windowStore} />
  {:else}
    <div class="none">
      <p class="title">{t("notes.none.title")}</p>
      <p class="text">{t("notes.none.text")}</p>
      <button type="button" class="btn btn--primary" onclick={() => void store.newNote()}>
        {t("notes.new")}
      </button>
    </div>
  {/if}
</div>

<VersionsDialog {store} />
<MoveDialog {store} />
<ConfirmDialog {store} />
<CompareDialog {store} />

<style>
  .page {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .none {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--sp-2);
    flex: 1;
    padding: var(--sp-8) var(--sp-5);
    text-align: center;
  }
  .none p {
    margin: 0;
  }
  .title {
    font: 600 var(--fs-reading) / var(--lh-reading) var(--font-reading);
  }
  .text {
    margin-bottom: var(--sp-3) !important;
    color: var(--muted);
  }
</style>
