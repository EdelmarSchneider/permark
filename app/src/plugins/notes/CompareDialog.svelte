<script lang="ts">
  // The text found after a crash against the note as it is on disk: removed lines in red, new ones in green, long
  // unchanged runs folded. The person keeps the recovered text or lets it go.
  import { diffRows } from "../../core/editing/diffRows";
  import { t } from "../../core/i18n";
  import NoteDialog from "./NoteDialog.svelte";
  import type { NotesStore } from "./store.svelte";

  type Props = { store: NotesStore };
  let { store }: Props = $props();

  const shown = $derived(store.compare);
  const rows = $derived(shown.status === "open" ? diffRows(shown.theirs, shown.mine) : []);
</script>

{#if shown.status === "open"}
  <NoteDialog title={t("notes.recovered.compareTitle")} close={() => store.closeCompare()} wide>
    <p class="legend">{t("notes.recovered.compareLegend")}</p>
    <div
      class="diff"
      tabindex="0"
      role="textbox"
      aria-readonly="true"
      aria-multiline="true"
      aria-label={t("notes.recovered.compareTitle")}
    >
      {#each rows as row (row.key)}
        {#if row.kind === "fold"}
          <div class="row fold">{t("conflict.folded", { count: row.count })}</div>
        {:else}
          <div class={["row", row.kind]}>
            <span class="sign" aria-hidden="true"
              >{row.kind === "add" ? "+" : row.kind === "del" ? "−" : ""}</span
            >
            <span class="text">{row.text === "" ? " " : row.text}</span>
          </div>
        {/if}
      {/each}
    </div>
    {#snippet footer()}
      <button type="button" class="btn" onclick={() => store.closeCompare()}>
        {t("common.close")}
      </button>
      <button
        type="button"
        class="btn"
        onclick={() => void store.discardRecovered(shown.status === "open" ? shown.path : "")}
      >
        {t("notes.recovered.discard")}
      </button>
      <button
        type="button"
        class="btn btn--primary"
        onclick={() => void store.keepRecovered(shown.status === "open" ? shown.path : "")}
      >
        {t("notes.recovered.keep")}
      </button>
    {/snippet}
  </NoteDialog>
{/if}

<style>
  .legend {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .diff {
    flex: 1;
    min-height: 0;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
  }
  .row {
    display: flex;
    gap: var(--sp-2);
    padding: 0 var(--sp-2);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .sign {
    flex: none;
    width: 1ch;
    color: var(--faint);
  }
  .add {
    background: var(--review-add-bg);
  }
  .add .sign {
    color: var(--git-added-text);
  }
  .del {
    background: var(--review-del-bg);
  }
  .del .sign {
    color: var(--git-deleted-text);
  }
  .fold {
    justify-content: center;
    color: var(--faint);
    font-family: var(--font-ui);
    font-size: var(--fs-ui-xs);
  }
</style>
