<script lang="ts">
  // The file changed (or went away) on disk while the person edited it: nothing is overwritten. The banner says
  // so and offers the three ways out; the differences (theirs -> mine) unfold below it.
  import { t } from "../i18n";
  import { diffRows, type DiffRow } from "./diffRows";

  type Props = {
    /** The disk's text; `null` while it is read; `deleted` when the file is gone. */
    theirs: string | null;
    deleted: boolean;
    mine: string;
    reload: () => void;
    keepMine: () => void;
    saveAsNew: () => void;
  };
  let { theirs, deleted, mine, reload, keepMine, saveAsNew }: Props = $props();

  let open = $state(false);
  let confirming = $state(false);

  const rows = $derived<DiffRow[]>(open && theirs !== null ? diffRows(theirs, mine) : []);
</script>

<div class="conflict" role="alert">
  <div class="banner">
    <p class="what">
      <strong>{deleted ? t("conflict.deletedTitle") : t("conflict.title")}</strong>
      {deleted ? t("conflict.deletedText") : t("conflict.text")}
    </p>
    <div class="actions">
      {#if confirming}
        <span class="confirm">{t("conflict.reloadConfirm")}</span>
        <button type="button" class="btn btn--sm btn--primary" onclick={reload}
          >{t("conflict.discard")}</button
        >
        <button type="button" class="btn btn--sm" onclick={() => (confirming = false)}
          >{t("common.cancel")}</button
        >
      {:else}
        {#if !deleted}
          <button type="button" class="btn btn--sm" onclick={() => (confirming = true)}>
            {t("conflict.reload")}
          </button>
        {/if}
        <button type="button" class="btn btn--sm btn--primary" onclick={keepMine}>
          {deleted ? t("conflict.recreate") : t("conflict.keepMine")}
        </button>
        <button type="button" class="btn btn--sm" onclick={saveAsNew}
          >{t("conflict.saveAsNew")}</button
        >
        {#if !deleted}
          <button
            type="button"
            class="btn btn--sm btn--ghost"
            aria-expanded={open}
            onclick={() => (open = !open)}
          >
            {open ? t("conflict.hideDiff") : t("conflict.showDiff")}
          </button>
        {/if}
      {/if}
    </div>
  </div>
  {#if open}
    <div class="diff" aria-label={t("conflict.diffLabel")}>
      {#if theirs === null}
        <p class="note">{t("doc.loading")}</p>
      {:else if rows.length === 0}
        <p class="note">{t("conflict.same")}</p>
      {:else}
        <p class="legend">
          <span class="del">− {t("conflict.theirs")}</span>
          <span class="add">+ {t("conflict.mine")}</span>
        </p>
        {#each rows as row (row.key)}
          {#if row.kind === "fold"}
            <div class="row fold">{t("conflict.folded", { count: row.count })}</div>
          {:else}
            <div class="row {row.kind}">
              <span class="mark">{row.kind === "add" ? "+" : row.kind === "del" ? "−" : " "}</span>
              <span class="text">{row.text}</span>
            </div>
          {/if}
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .conflict {
    position: sticky;
    top: 0;
    z-index: 7;
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--git-modified) 10%, var(--bg));
  }
  .banner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2) var(--sp-4);
    padding: var(--sp-2) var(--sp-4);
  }
  .what {
    flex: 1 1 280px;
    margin: 0;
    font-size: var(--fs-ui);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
  }
  .confirm {
    font-size: var(--fs-ui-sm);
    color: var(--muted);
  }
  .diff {
    max-height: 45vh;
    overflow: auto;
    padding: var(--sp-2) 0;
    border-top: 1px solid var(--border);
    background: var(--bg);
    font: var(--fs-code) / 1.5 var(--font-code);
  }
  .legend {
    display: flex;
    gap: var(--sp-4);
    margin: 0 0 var(--sp-2);
    padding: 0 var(--sp-4);
    font: 500 var(--fs-ui-sm) / 1.4 var(--font-ui);
  }
  .note {
    margin: 0;
    padding: 0 var(--sp-4);
    color: var(--muted);
    font: var(--fs-ui) / 1.4 var(--font-ui);
  }
  .row {
    display: flex;
    padding: 0 var(--sp-4);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .mark {
    flex: 0 0 1.5em;
    color: var(--faint);
    user-select: none;
  }
  .row.add,
  .legend .add {
    background: var(--review-add-bg);
    color: var(--git-added-text);
  }
  .row.del,
  .legend .del {
    background: var(--review-del-bg);
    color: var(--git-deleted-text);
  }
  .row.fold {
    color: var(--faint);
    font-style: italic;
    font-family: var(--font-ui);
  }
</style>
