<script lang="ts">
  // The strips over the note shown: text found after a crash (Compare | Keep | Discard), and the state of a loose
  // file (changed on disk | gone | unsaved) with its ways out.
  import { t } from "../../core/i18n";
  import { shownName } from "./model";
  import type { NotesStore } from "./store.svelte";

  type Props = { store: NotesStore };
  let { store }: Props = $props();

  const entry = $derived(store.entry);
  const recovered = $derived(entry !== null && entry.kind === "note" && entry.recovered);
  /** A loose file that is not open: its state is told. An open one has the editor's own conflict view. */
  const file = $derived(entry !== null && entry.kind === "file" ? entry : null);
  const gone = $derived(file !== null && file.state === "gone");
  const unsaved = $derived(file !== null && file.state === "unsaved");
  const changed = $derived(file !== null && file.state === "changedOnDisk");
</script>

{#if entry !== null && recovered}
  <div class="bar recovered" role="status" data-bar="recovered">
    <p class="what">
      <strong>{t("notes.recovered.title")}</strong>
      {t("notes.recovered.text")}
    </p>
    <div class="actions">
      <button type="button" class="btn btn--sm" onclick={() => void store.openCompare(entry.path)}>
        {t("notes.recovered.compare")}
      </button>
      <button
        type="button"
        class="btn btn--sm"
        onclick={() => void store.discardRecovered(entry.path)}
      >
        {t("notes.recovered.discard")}
      </button>
      <button
        type="button"
        class="btn btn--sm btn--primary"
        onclick={() => void store.keepRecovered(entry.path)}
      >
        {t("notes.recovered.keep")}
      </button>
    </div>
  </div>
{/if}

{#if file !== null && gone}
  <div class="bar gone" role="status" data-bar="gone">
    <p class="what">
      <strong>{t("notes.file.gone", { name: file.name })}</strong>
      {#if file.pending}{t("notes.file.goneKept")}{:else}{t("notes.file.missingText", {
          folder: file.folder ?? "",
        })}{/if}
    </p>
    <div class="actions">
      {#if file.pending}
        <button type="button" class="btn btn--sm" onclick={() => void store.saveAsNote(file.path)}>
          {t("notes.file.saveAsNote")}
        </button>
        <button type="button" class="btn btn--sm" onclick={() => void store.keepMine(file.path)}>
          {t("notes.file.saveAgain")}
        </button>
      {/if}
      <button type="button" class="btn btn--sm" onclick={() => void store.locateFile(file.path)}>
        {t("notes.file.locate")}
      </button>
      <button type="button" class="btn btn--sm" onclick={() => void store.removeFile(file.path)}>
        {t("notes.file.remove")}
      </button>
    </div>
  </div>
{:else if file !== null && unsaved}
  <div class="bar unsaved" role="status" data-bar="unsaved">
    <p class="what">
      <strong>{t("notes.file.unsaved")}</strong>
      {t("notes.file.changedKept")}
    </p>
    <div class="actions">
      <button type="button" class="btn btn--sm" onclick={() => void store.reloadFile(file.path)}>
        {t("notes.file.reload")}
      </button>
      <button type="button" class="btn btn--sm" onclick={() => void store.saveAsNote(file.path)}>
        {t("notes.file.saveAsNote")}
      </button>
      <button
        type="button"
        class="btn btn--sm btn--primary"
        onclick={() => void store.keepMine(file.path)}
      >
        {t("notes.file.keepMine")}
      </button>
    </div>
  </div>
{:else if file !== null && changed && store.current !== file.path}
  <div class="bar changed" role="status" data-bar="changed">
    <p class="what"><strong>{t("notes.file.changed")}</strong></p>
  </div>
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--sp-2) var(--sp-4);
    padding: var(--sp-2) var(--sp-4);
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--git-modified) 9%, var(--bg));
    font-size: var(--fs-ui);
  }
  .gone {
    background: color-mix(in srgb, var(--git-deleted) 8%, var(--bg));
  }
  .what {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
  }
</style>
