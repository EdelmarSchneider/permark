<script lang="ts">
  // The earlier versions of a document: the commits that changed it, newest first. A row says when, what (the first
  // line of the commit), the exact moment and who; picking one compares the document with that version. Keys in the
  // rows: Up | Down | Home | End move (they stop at the ends: rows past the last one are asked for with "Show more"),
  // Enter | Space pick; Esc closes. What the repository says (subjects, names, an old path) is drawn as text only.
  import X from "@lucide/svelte/icons/x";
  import { tick } from "svelte";
  import type { VersionRow } from "../../api";
  import { isComposition } from "../../core/commands/combo";
  import { documents } from "../../core/doc/document.svelte";
  import { language, t } from "../../core/i18n";
  import { joinNames } from "../../core/projects/model";
  import { dialogFocus } from "../../core/shell/dialogFocus";
  import { exactTime, relativeTime } from "../../core/time";
  import { problemText, review, type VersionsUi } from "./store.svelte";

  type Props = { list: Extract<VersionsUi, { status: "open" }> };
  let { list }: Props = $props();

  /** Prefix of the ids the parts name each other by. */
  const ID = "review-versions";
  /** The moment the ages of the rows are told from: when the list was asked for. */
  const now = Date.now();

  let dialog: HTMLElement | undefined = $state();
  let listbox: HTMLElement | undefined = $state();
  /** Index of the row the keyboard was put on. */
  let active = $state(0);

  const rows = $derived(list.rows);
  const name = $derived(list.path.split("/").at(-1) ?? list.path);
  /** The row the keyboard is on, in the rows there are now (a list read again may be shorter). */
  const at = $derived(Math.max(0, Math.min(active, rows.length - 1)));
  /** A pick is for the document on screen: while the window is on its way to this file, the page shows another. */
  const ready = $derived(documents.view?.path === list.path);
  /** Reading again can help: "Try again" reads what "Show more" would. */
  const retryable = $derived(list.problem?.kind === "slow" || list.problem?.kind === "failed");
  const canMore = $derived(list.next !== null && !retryable);

  const rowId = (index: number): string => `${ID}-row-${index}`;

  function who(row: VersionRow): string {
    return row.coAuthors.length === 0
      ? row.author
      : t("review.versions.authors", {
          author: row.author,
          others: joinNames(row.coAuthors, language.locale),
        });
  }

  function close(): void {
    review.closeVersions();
  }

  /** The row the keyboard is on is brought into view. */
  function showActive(): void {
    document.getElementById(rowId(at))?.scrollIntoView({ block: "nearest" });
  }

  async function pick(row: VersionRow | undefined): Promise<void> {
    if (row === undefined || row.same || !ready) return;
    await review.pickVersion(row);
    // the list is still open: a line under the rows says why, and took room from them
    await tick();
    showActive();
  }

  /** A button that goes away with what it asks for was pressed: the keyboard waits in the rows (the dialog, with
   *  none), so Esc and Tab still belong to the dialog. */
  function hold(): void {
    (listbox ?? dialog)?.focus();
  }

  async function more(): Promise<void> {
    const before = rows.length;
    hold();
    await review.moreVersions();
    // the keyboard goes on where the list went on
    if (rows.length > before) active = before;
  }

  function retry(): void {
    hold();
    void review.retryVersions();
  }

  function onListKey(event: KeyboardEvent): void {
    if (isComposition(event) || event.altKey || event.ctrlKey || event.metaKey) return;
    const last = rows.length - 1;
    let to: number;
    switch (event.key) {
      case "ArrowDown":
        to = Math.min(at + 1, last);
        break;
      case "ArrowUp":
        to = Math.max(at - 1, 0);
        break;
      case "Home":
        to = 0;
        break;
      case "End":
        to = last;
        break;
      case "Enter":
      case " ":
        event.preventDefault();
        void pick(rows[at]);
        return;
      default:
        return;
    }
    event.preventDefault();
    active = to;
  }

  // the rows came after the dialog opened: the keyboard goes to them, unless the person moved it meanwhile
  $effect(() => {
    if (rows.length > 0 && listbox && document.activeElement === dialog) listbox.focus();
  });

  // the row the keyboard is on stays in view
  $effect(showActive);
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) close();
  }}
>
  <!-- with no rows yet the dialog itself takes the focus: its title is read first -->
  <div
    bind:this={dialog}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{ID}-title"
    tabindex="-1"
    use:dialogFocus={{
      initial: (d) => d.querySelector<HTMLElement>('[role="listbox"]') ?? d,
      onEscape: close,
    }}
  >
    <header>
      <h2 id="{ID}-title">{t("review.versions.title", { name })}</h2>
      <button
        type="button"
        class="icon-btn icon-btn--sm"
        data-action="close"
        aria-label={t("common.close")}
        title={t("common.close")}
        onclick={close}
      >
        <X size={16} />
      </button>
    </header>

    <div class="body" aria-busy={list.loading}>
      {#if rows.length > 0}
        <div
          bind:this={listbox}
          class="list"
          role="listbox"
          tabindex="0"
          aria-label={t("review.versions.list")}
          aria-activedescendant={rowId(at)}
          onkeydown={onListKey}
        >
          {#each rows as row, index (row.commit)}
            {@const off = row.same || !ready}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              id={rowId(index)}
              class="row"
              class:active={index === at}
              class:off
              class:dim={row.same}
              role="option"
              tabindex="-1"
              aria-selected={index === at}
              aria-disabled={off ? "true" : undefined}
              onclick={() => {
                active = index;
                void pick(row);
              }}
            >
              <span class="line">
                <span class="when">{relativeTime(row.atMs, now, language.locale)}</span>
                <span class="subject" dir="auto" title={row.subject}>{row.subject}</span>
              </span>
              <span class="facts">
                <span class="exact">{exactTime(row.atMs, language.locale)}</span>
                <span class="who">{who(row)}</span>
                {#if row.same}<span class="same">{t("review.versions.same")}</span>{/if}
              </span>
              {#if !off}<span class="act">{t("review.versions.compare")}</span>{/if}
            </div>
          {/each}
        </div>
      {/if}

      {#if canMore || list.loading}
        <div class="next">
          {#if canMore}
            <button
              type="button"
              class="btn btn--sm"
              data-action="more"
              disabled={list.loading}
              onclick={() => void more()}
            >
              {t("review.versions.more")}
            </button>
          {/if}
          {#if list.loading}
            <span class="state" role="status">{t("review.versions.loading")}</span>
          {/if}
        </div>
      {/if}

      {#if list.problem !== null}
        <div class="next">
          <p class="problem" role="alert">{problemText(list.problem)}</p>
          {#if retryable}
            <button type="button" class="btn btn--sm" data-action="retry" onclick={retry}>
              {t("review.versions.retry")}
            </button>
          {/if}
        </div>
      {/if}

      <!-- always there (out of the layout while it says nothing), so that a line that comes (how a pick went) is
           said by a screen reader -->
      <p class="note" class:sr-only={list.note === null} role="status">{list.note ?? ""}</p>
      {#if list.renamedFrom !== null || list.cut}
        <div class="notes">
          {#if list.renamedFrom !== null}
            <p>{t("review.versions.renamed", { name: list.renamedFrom })}</p>
          {/if}
          {#if list.cut}<p>{t("review.versions.cut")}</p>{/if}
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: var(--sp-4);
    background: var(--backdrop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    width: min(520px, 100%);
    /* of the window, not of the backdrop's row: a percentage there is unknown while the row is sized, and a long
       list would make the row (and the place of the dialog in it) as tall as all its rows */
    max-height: min(640px, calc(100vh - 2 * var(--sp-4)));
    padding: var(--sp-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    color: var(--text);
    box-shadow: var(--shadow-pop);
    outline: none;
    font-size: var(--fs-ui);
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--sp-3);
  }
  h2 {
    min-width: 0;
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  header button {
    flex: none;
  }
  p {
    margin: 0;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    min-height: 0;
  }
  .list {
    flex: 0 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  .list:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    column-gap: var(--sp-2);
    padding: 6px var(--sp-2);
    border-radius: var(--radius-control);
    cursor: pointer;
  }
  .row:not(.off):hover {
    background: var(--hover);
  }
  .row.active {
    background: var(--accent-soft);
  }
  .row.off {
    cursor: default;
  }
  .line {
    grid-column: 1;
    grid-row: 1;
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    min-width: 0;
  }
  .when {
    flex: none;
    font-weight: 600;
  }
  .subject {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row.dim .when,
  .row.dim .subject {
    color: var(--muted);
  }
  .facts {
    grid-column: 1;
    grid-row: 2;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px var(--sp-2);
    min-width: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .who {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .same {
    padding: 0 6px;
    border-radius: 8px;
    background: var(--surface-2);
  }
  /* what a pick does, on the row the keyboard | the pointer is on (kept in the layout: the text never jumps) */
  .act {
    grid-column: 2;
    grid-row: 1 / span 2;
    visibility: hidden;
    color: var(--accent-text);
    font-size: var(--fs-ui-sm);
    font-weight: 500;
  }
  .row.active .act,
  .row:hover .act {
    visibility: visible;
  }
  .next {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
  }
  .state,
  .notes {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .problem,
  .note,
  .notes p {
    overflow-wrap: anywhere;
  }
  .notes {
    display: grid;
    gap: 2px;
  }
</style>
