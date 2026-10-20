<script lang="ts">
  // Earlier versions of a note: the list (newest first), the text of the one picked, Restore. The text the note has
  // now is kept as a version first, so a restore can be taken back.
  import { t, tn } from "../../core/i18n";
  import { shownName } from "./model";
  import NoteDialog from "./NoteDialog.svelte";
  import type { NotesStore } from "./store.svelte";

  type Props = { store: NotesStore };
  let { store }: Props = $props();

  const shown = $derived(store.versions);
  const entry = $derived(
    shown.status === "open" ? (store.entries.find((e) => e.path === shown.path) ?? null) : null,
  );

  function when(ms: number): string {
    return new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
  }

  function size(bytes: number): string {
    return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(bytes < 10240 ? 1 : 0)} KB`;
  }
</script>

{#if shown.status === "open"}
  <NoteDialog
    title={t("notes.versions.of", { name: entry ? shownName(entry) : shown.path })}
    close={() => store.closeVersions()}
    wide
  >
    {#if shown.items === null}
      <p class="faint">{t("notes.versions.loading")}</p>
    {:else if shown.items.length === 0}
      <p class="faint">{t("notes.versions.empty")}</p>
    {:else}
      <div class="split">
        <ul class="list" role="listbox" aria-label={t("notes.versions.list")}>
          {#each shown.items as item (item.id)}
            <li>
              <button
                type="button"
                class="item"
                role="option"
                aria-selected={shown.selected === item.id}
                data-first={shown.selected === item.id ? "" : undefined}
                onclick={() => void store.selectVersion(item.id)}
              >
                <span>{when(item.atMs)}</span>
                <span class="faint">{size(item.size)}</span>
              </button>
            </li>
          {/each}
        </ul>
        <div
          class="preview"
          tabindex="0"
          role="textbox"
          aria-readonly="true"
          aria-multiline="true"
          aria-label={t("notes.versions.preview")}
        >
          {shown.preview ?? t("notes.versions.loading")}
        </div>
      </div>
      <p class="faint hint">{t("notes.versions.restoreHint")}</p>
    {/if}
    {#snippet footer()}
      <span class="count faint">{tn("notes.versions.count", shown.items?.length ?? 0)}</span>
      <button type="button" class="btn" onclick={() => store.closeVersions()}>
        {t("common.close")}
      </button>
      <button
        type="button"
        class="btn btn--primary"
        disabled={shown.selected === null}
        onclick={() => void store.restoreVersion()}
      >
        {t("notes.versions.restore")}
      </button>
    {/snippet}
  </NoteDialog>
{/if}

<style>
  .faint {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .split {
    display: grid;
    grid-template-columns: 220px 1fr;
    gap: var(--sp-3);
    flex: 1;
    min-height: 0;
  }
  .list {
    margin: 0;
    padding: 0;
    list-style: none;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  .item {
    display: flex;
    justify-content: space-between;
    gap: var(--sp-2);
    width: 100%;
    padding: 6px 10px;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui);
    text-align: left;
    cursor: pointer;
  }
  .item:hover {
    background: var(--hover);
  }
  .item[aria-selected="true"] {
    background: var(--accent-soft);
  }
  .preview {
    margin: 0;
    padding: var(--sp-3);
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .hint {
    padding: 0 2px;
  }
  .count {
    margin-right: auto;
    align-self: center;
  }
</style>
