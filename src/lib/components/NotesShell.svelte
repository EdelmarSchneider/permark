<script lang="ts">
  // The Notes window (docs/design/v0.5.md section 2): the list of notes on the left, the note on the right, one status bar.
  // It reuses the document view and the two editors of the project window; what belongs to a project (git, unread markers,
  // the right panel, the agent connection) is not here.
  import { t } from "../i18n";
  import { pickFolder } from "../platform";
  import { edit } from "../stores/edit.svelte";
  import { notes } from "../stores/notes.svelte";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import { bridge } from "../bridge";
  import DocView from "./DocView.svelte";
  import EditBars from "./EditBars.svelte";
  import FindBar from "./FindBar.svelte";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Notice from "./Notice.svelte";
  import NotesActivityBar from "./NotesActivityBar.svelte";
  import NotesHeader from "./NotesHeader.svelte";
  import NotesList from "./NotesList.svelte";
  import NotesStatusBar from "./NotesStatusBar.svelte";
  import Resizer from "./Resizer.svelte";
  import SourceEditor from "./SourceEditor.svelte";
  import Toast from "./Toast.svelte";
  import VisualEditor from "./VisualEditor.svelte";

  async function locate(): Promise<void> {
    try {
      const path = await pickFolder(t("dialog.chooseNotesFolder"));
      if (path) await notes.locate(path);
    } catch {
      // the dialog failed or was dismissed: the folder is still missing
    }
  }
</script>

{#if notes.phase === "ready"}
  <div
    class={["app", "app--notes", ui.resizing && "is-resizing"]}
    data-sidebar={session.left.open ? "shown" : "hidden"}
    style:--sb="{session.left.open ? session.left.width : 0}px"
  >
    <NotesActivityBar />
    <aside class="sidebar" aria-label={t("notes.title")} inert={!session.left.open}>
      <Resizer side="left" />
      <div class="panel-inner" style:width="{session.left.width}px">
        <NotesList />
      </div>
    </aside>
    <main class="main" data-width={session.width}>
      <NotesHeader />
      {#if notes.readOnly}
        <div class="banner banner--info" role="status">
          <Icon name="lock" />
          <div>{t(`notes.readOnly.${notes.readOnly}`)}</div>
        </div>
      {/if}
      {#if edit.active}<EditBars />{/if}
      <DocView />
      {#if edit.active}
        <!-- one editor per note and mode: opening another note, or the other mode, starts a fresh one -->
        {#key `${edit.path}:${edit.mode}`}
          {#if edit.mode === "visual"}<VisualEditor />{:else}<SourceEditor />{/if}
        {/key}
      {/if}
      {#if !notes.current}
        <div class="notes-none">
          <Notice icon="sticky-note" title={t("notes.none.title")} text={t("notes.none.text")}>
            {#if !notes.readOnly}
              <button type="button" class="btn btn--primary" onclick={() => void notes.newNote()}>
                <Icon name="plus" size="sm" />{t("notes.new")}
              </button>
            {/if}
          </Notice>
        </div>
      {/if}
      {#if ui.find.open && !edit.active}<FindBar />{/if}
      <Toast />
    </main>
    <NotesStatusBar />
  </div>
{:else if notes.phase === "missing"}
  <div class="welcome notes-problem">
    <Notice
      icon="folder-search"
      title={t("notes.folderMissing", { path: notes.folder?.path ?? "" })}
      text={t("notes.folderMissingHint")}
    >
      <button type="button" class="btn btn--primary" onclick={() => void locate()}>
        <Icon name="folder-open" size="sm" />{t("notes.locate")}
      </button>
      <button type="button" class="btn" onclick={() => void notes.createFolder()}>
        <Icon name="plus" size="sm" />{t("notes.createFolder")}
      </button>
    </Notice>
    <Toast />
  </div>
{:else if notes.phase === "failed"}
  <div class="welcome notes-problem">
    <Notice icon="circle-alert" title={t("notes.failed", { message: notes.failure ?? "" })} text="">
      <button type="button" class="btn btn--primary" onclick={() => void notes.start()}>
        {t("notes.retry")}
      </button>
    </Notice>
  </div>
{:else}
  <div class="welcome" aria-busy="true" aria-label={t("app.loading")}></div>
{/if}
