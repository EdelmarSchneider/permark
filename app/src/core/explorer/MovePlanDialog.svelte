<script lang="ts">
  // "Update links?": what a move would change (links by document, with line and old -> new), what could not be looked
  // at, and the answers: move and update | move only | cancel. Escape | backdrop = cancel.
  import type { MovePlan, SkipReason } from "../../api";
  import { t, tn, type MessageKey } from "../i18n";

  type Props = { plan: MovePlan; answer: (choice: "update" | "move" | "cancel") => void };
  let { plan, answer }: Props = $props();

  const REASONS: Record<SkipReason, MessageKey> = {
    tooLarge: "fileOps.reason.tooLarge",
    notUtf8: "fileOps.reason.notUtf8",
    unreadable: "fileOps.reason.unreadable",
    changed: "fileOps.reason.changed",
    writeFailed: "fileOps.reason.writeFailed",
    missing: "fileOps.reason.missing",
    occupied: "fileOps.reason.occupied",
    unsaved: "fileOps.reason.unsaved",
    trash: "fileOps.reason.trash",
  };

  /** Moves listed before "and n more". */
  const SHOWN_MOVES = 4;

  let dialog: HTMLElement | undefined = $state();
  let primary: HTMLButtonElement | undefined = $state();

  const hasLinks = $derived(plan.linkCount > 0);
  /** The ignored folders whose documents were not read: the first few, then "…". */
  const uncheckedText = $derived(
    plan.unchecked.slice(0, 3).join(", ") + (plan.unchecked.length > 3 ? ", …" : ""),
  );
  const title = $derived(
    hasLinks
      ? tn("fileOps.updateLinks", plan.linkCount, {
          files: tn("fileOps.files", plan.fileCount),
        })
      : t("fileOps.dialogLabel"),
  );

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      answer("cancel");
      return;
    }
    if (e.key !== "Tab" || !dialog) return;
    // focus stays inside the dialog
    const focusable = Array.from(dialog.querySelectorAll<HTMLElement>("button:not(:disabled)"));
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last?.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first?.focus();
    }
  }

  // focus the default answer once shown; give focus back when closed
  $effect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    primary?.focus();
    return () => before?.focus();
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) answer("cancel");
  }}
>
  <div
    bind:this={dialog}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="plan-title"
    tabindex="-1"
    {onkeydown}
  >
    <h2 id="plan-title">{title}</h2>
    <section aria-label={t("fileOps.movesLabel")} class="moves">
      {#each plan.moves.slice(0, SHOWN_MOVES) as move (move.from)}
        <p><code>{move.from}</code> <span class="arrow">→</span> <code>{move.to}</code></p>
      {/each}
      {#if plan.moves.length > SHOWN_MOVES}
        <p class="faint">{tn("fileOps.moreMoves", plan.moves.length - SHOWN_MOVES)}</p>
      {/if}
    </section>
    {#if hasLinks}
      <p class="intro">{t("fileOps.updateIntro")}</p>
      <div class="list">
        {#each plan.files as file (file.path)}
          <section class="doc">
            <h3>
              {file.path}
              {#if file.movedTo}<span class="faint"
                  >{t("fileOps.movesTo", { path: file.movedTo })}</span
                >{/if}
            </h3>
            <ul>
              {#each file.changes as change, at (at)}
                <li>
                  <span class="faint">{t("fileOps.atLine", { line: change.line })}</span>
                  <code class="old">{change.old}</code>
                  <span class="arrow">→</span>
                  <code class="new">{change.new}</code>
                </li>
              {/each}
            </ul>
          </section>
        {/each}
        {#if plan.truncated}<p class="faint">{t("fileOps.moreLinks")}</p>{/if}
      </div>
    {/if}
    {#if plan.skipped.length > 0}
      <section class="skipped">
        <h3>{t("fileOps.skippedTitle")}</h3>
        <ul>
          {#each plan.skipped as skip (skip.path)}
            <li>{skip.path} <span class="faint">{t(REASONS[skip.reason])}</span></li>
          {/each}
        </ul>
      </section>
    {/if}
    {#if plan.unchecked.length > 0}
      <p class="faint" data-unchecked>
        {tn("fileOps.unchecked", plan.unchecked.length, { folders: uncheckedText })}
      </p>
    {/if}
    <footer>
      <button type="button" class="btn" onclick={() => answer("cancel")}
        >{t("common.cancel")}</button
      >
      {#if hasLinks}
        <button type="button" class="btn" onclick={() => answer("move")}
          >{t("fileOps.moveOnly")}</button
        >
        <button
          bind:this={primary}
          type="button"
          class="btn btn--primary"
          onclick={() => answer("update")}
        >
          {t("fileOps.moveAndUpdate")}
        </button>
      {:else}
        <button
          bind:this={primary}
          type="button"
          class="btn btn--primary"
          onclick={() => answer("move")}
        >
          {t("fileOps.move")}
        </button>
      {/if}
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: var(--sp-5);
    background: var(--backdrop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    width: min(620px, 100%);
    max-height: min(640px, 100%);
    padding: var(--sp-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  h3 {
    margin: 0 0 var(--sp-1);
    font-size: var(--fs-ui);
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  p {
    margin: 0;
  }
  .moves p {
    overflow-wrap: anywhere;
  }
  .intro,
  .faint {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    min-height: 0;
    overflow: auto;
    padding: var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--sp-2);
    padding: 2px 0;
    font-size: var(--fs-ui-sm);
  }
  code {
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .old {
    color: var(--git-deleted-text);
  }
  .new {
    color: var(--git-added-text);
  }
  .arrow {
    color: var(--faint);
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
</style>
