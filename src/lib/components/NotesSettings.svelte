<script lang="ts">
  // Settings -> Notes: where the notes are, a button to open the folder, and where TrailMark opens when it is started with nothing
  // to open. Shown in every window; they are app-wide, not part of a project.
  import { onMount } from "svelte";
  import { api } from "../api";
  import { errorMessage } from "../errors";
  import { t } from "../i18n";
  import { pickFolder } from "../platform";
  import { app } from "../stores/app.svelte";
  import { notes } from "../stores/notes.svelte";
  import { toast } from "../stores/toast.svelte";
  import type { NotesFolder, StartIn } from "../types";

  const CHOICES: StartIn[] = ["welcome", "notes"];

  let folder = $state.raw<NotesFolder | null>(null);
  let startIn = $state<StartIn>("welcome");

  onMount(() => {
    void api.notesFolder().then((value) => (folder = value));
    void api.notesStartIn().then((value) => (startIn = value));
  });

  async function change(): Promise<void> {
    try {
      const path = await pickFolder(t("dialog.chooseNotesFolder"));
      if (!path) return;
      // the Notes window opens the new folder at once; another window keeps it for the next time the Notes window opens
      if (app.screen === "notes") await notes.locate(path);
      else folder = await api.setNotesFolder(path);
      folder = await api.notesFolder();
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }

  async function open(): Promise<void> {
    try {
      await api.openNotesFolder();
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }

  async function choose(choice: StartIn): Promise<void> {
    startIn = choice;
    try {
      await api.setNotesStartIn(choice);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }
</script>

<div class="field">
  <span class="field__label">{t("settings.notes")}</span>
  <span class="field__preview" aria-live="polite" title={folder?.path}>
    {folder ? folder.path : ""}
  </span>
  {#if folder?.status === "missing"}
    <span class="field__status field__status--error" role="status">
      {t("notes.folderMissing", { path: folder.path })}
    </span>
  {/if}
  <div class="field__row">
    <button type="button" class="btn btn--sm" onclick={() => void change()}>
      {t("settings.notesFolderChange")}
    </button>
    <button
      type="button"
      class="btn btn--sm"
      disabled={folder?.status !== "ready"}
      onclick={() => void open()}
    >
      {t("notes.openFolder")}
    </button>
  </div>
  <span class="field__status">{t("settings.notesFolderHint")}</span>
</div>
<div class="field">
  <span class="field__label">{t("settings.startIn")}</span>
  <div class="segmented" role="group" aria-label={t("settings.startIn")}>
    {#each CHOICES as choice (choice)}
      <button type="button" aria-pressed={startIn === choice} onclick={() => void choose(choice)}>
        {t(`settings.startIn.${choice}`)}
      </button>
    {/each}
  </div>
  <span class="field__status">{t("settings.startInHint")}</span>
</div>
<div class="field">
  <span class="field__label">{t("settings.files")}</span>
  <label class="filter__check">
    <input
      type="checkbox"
      checked={notes.autoReload}
      aria-describedby="auto-reload-hint"
      onchange={(e) => notes.setAutoReload(e.currentTarget.checked)}
    />
    <span class="filter__text">
      <span>{t("settings.autoReload")}</span>
      <span id="auto-reload-hint" class="filter__hint">{t("settings.autoReloadHint")}</span>
    </span>
  </label>
</div>
