<script lang="ts">
  // The Notes on the welcome screen: a new note, the pinned ones and the latest five. Clicking opens the Notes with
  // that note shown.
  import FilePlus from "@lucide/svelte/icons/file-plus";
  import type { NotesSummary } from "../../api";
  import { t } from "../../core/i18n";
  import { pluginClient } from "../../core/plugins/client";
  import { windowStore } from "../../core/window.svelte";
  import { shownName, timeLabel } from "./model";
  import { wantNewNote, wantNote } from "./store.svelte";

  let summary = $state.raw<NotesSummary | null>(null);

  // read once when the welcome screen appears
  $effect(() => {
    let gone = false;
    void pluginClient.call("notes.summary", {}).then((result) => {
      if (!gone && result.ok) summary = result.value;
    });
    return () => {
      gone = true;
    };
  });

  const now = Date.now();

  async function openNotes(path: string | null): Promise<void> {
    if (path === null) wantNewNote();
    else wantNote(path);
    await windowStore.openNotes("ask");
  }
</script>

<section class="notes" aria-label={t("notes.title")}>
  <div class="head">
    <h2>{t("notes.title")}</h2>
    <button type="button" class="btn btn--sm" onclick={() => void openNotes(null)}>
      <FilePlus size={13} aria-hidden="true" />
      {t("notes.new")}
    </button>
  </div>
  {#if summary !== null && summary.pinned.length + summary.latest.length > 0}
    <ul>
      {#each [...summary.pinned, ...summary.latest] as entry (entry.path)}
        <li>
          <button type="button" class="row" onclick={() => void openNotes(entry.path)}>
            <span class="title" class:untitled={entry.title === ""}>
              {entry.title === "" ? t("notes.untitled") : shownName(entry)}
            </span>
            <span class="time">{timeLabel(entry.modifiedMs, now)}</span>
          </button>
        </li>
      {/each}
    </ul>
  {:else if summary !== null}
    <p class="empty">{t("notes.welcome.none")}</p>
  {/if}
</section>

<style>
  .notes {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-left: var(--sp-3);
  }
  h2 {
    margin: 0;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--sp-3);
    width: 100%;
    padding: 5px 10px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui);
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--hover);
  }
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .untitled {
    color: var(--muted);
    font-style: italic;
  }
  .time {
    flex: none;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
  }
  .empty {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
</style>
