<script lang="ts">
  // Move a note into a project: pick the folder (a recent project or Browse) and the file name; the note's exact text
  // is copied there (never over a file), then the note leaves the notes (undo is offered).
  import { t } from "../../core/i18n";
  import { nextRow } from "./model";
  import NoteDialog from "./NoteDialog.svelte";
  import type { NotesStore } from "./store.svelte";

  type Props = { store: NotesStore };
  let { store }: Props = $props();

  const shown = $derived(store.move);
  const recents = $derived(shown.status === "open" ? shown.recents : []);
  /** The folder is one of the recent projects (else it was browsed to). */
  const browsed = $derived(
    shown.status === "open" &&
      shown.folder !== null &&
      !recents.some((r) => r.path === shown.folder),
  );
  /** The one row of the list in the Tab order: the chosen folder, else the first (radio group: arrows move in it). */
  const tabStop = $derived(
    shown.status !== "open" || browsed
      ? null
      : recents.some((r) => r.path === shown.folder)
        ? shown.folder
        : (recents[0]?.path ?? null),
  );
  const MOVES = new Set(["ArrowDown", "ArrowRight", "ArrowUp", "ArrowLeft", "Home", "End"]);

  /** Arrows | Home | End move the choice through the list (it follows the focus, which stays in view). */
  function onListKey(e: KeyboardEvent): void {
    if (!MOVES.has(e.key) || e.ctrlKey || e.metaKey || e.altKey) return;
    const list = e.currentTarget;
    if (!(list instanceof HTMLElement)) return;
    const rows = Array.from(list.querySelectorAll<HTMLElement>("[role='radio']"));
    if (rows.length === 0) return;
    e.preventDefault();
    const at = rows.findIndex((r) => r === document.activeElement);
    const row = rows[nextRow(at, rows.length, e.key)];
    if (!row) return;
    row.focus();
    row.scrollIntoView({ block: "nearest" });
    row.click();
  }
</script>

{#if shown.status === "open"}
  <NoteDialog title={t("notes.move.title")} close={() => store.closeMove()}>
    <div class="group">
      <span class="label" id="notes-move-destination">{t("notes.move.destination")}</span>
      {#if recents.length > 0 || browsed}
        <!-- the list scrolls inside the dialog: many recent folders never push the buttons out -->
        <div
          class="list"
          role="radiogroup"
          tabindex="-1"
          aria-labelledby="notes-move-destination"
          onkeydown={onListKey}
        >
          {#each recents as recent (recent.path)}
            <button
              type="button"
              class="choice"
              role="radio"
              aria-checked={shown.folder === recent.path}
              tabindex={tabStop === recent.path ? 0 : -1}
              title={recent.path}
              onclick={() => void store.chooseMoveFolder(recent.path)}
            >
              <span class="name">{recent.name}</span>
              <span class="path">{recent.path}</span>
            </button>
          {/each}
          {#if browsed && shown.folder !== null}
            <button
              type="button"
              class="choice"
              role="radio"
              aria-checked="true"
              tabindex="0"
              title={shown.folder}
              data-skip
            >
              <span class="name">{shown.folder.split(/[\\/]/).filter(Boolean).at(-1)}</span>
              <span class="path">{shown.folder}</span>
            </button>
          {/if}
        </div>
      {:else}
        <p class="faint">{t("notes.move.noRecents")}</p>
      {/if}
      <button
        type="button"
        class="btn btn--sm browse"
        onclick={() => void store.browseMoveFolder()}
      >
        {t("notes.move.browse")}
      </button>
    </div>
    <label class="field">
      <span class="label">{t("notes.move.name")}</span>
      <input
        type="text"
        data-first
        value={shown.name}
        spellcheck="false"
        autocomplete="off"
        oninput={(e) => store.setMoveName(e.currentTarget.value)}
        onkeydown={(e) => {
          if (e.key === "Enter" && !e.isComposing) {
            e.preventDefault();
            void store.confirmMove();
          }
        }}
      />
    </label>
    {#if shown.error}<p class="error" role="alert">{shown.error}</p>{/if}
    {#snippet footer()}
      <button type="button" class="btn" onclick={() => store.closeMove()}>
        {t("common.cancel")}
      </button>
      <button
        type="button"
        class="btn btn--primary"
        disabled={shown.folder === null || shown.name.trim() === "" || shown.busy}
        onclick={() => void store.confirmMove()}
      >
        {t("notes.move.confirm")}
      </button>
    {/snippet}
  </NoteDialog>
{/if}

<style>
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    /* shrinks with the dialog (its list scrolls); the name field and the buttons keep their place */
    flex: 0 1 auto;
    min-height: 0;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    /* room for the focus ring (2px + 1px offset) inside the scroller */
    margin: -3px;
    padding: 3px;
  }
  .list:focus {
    outline: none;
  }
  .label {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .choice {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .choice:hover {
    background: var(--hover);
  }
  .choice[aria-checked="true"] {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .name {
    font-size: var(--fs-ui);
    font-weight: 500;
  }
  .path {
    max-width: 100%;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .browse {
    flex-shrink: 0;
    align-self: flex-start;
    margin-top: var(--sp-1);
  }
  .field {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  input {
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui);
  }
  input:focus {
    outline: 2px solid var(--accent-muted);
    outline-offset: -1px;
  }
  .faint {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .error {
    margin: 0;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-sm);
  }
</style>
