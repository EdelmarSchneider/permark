<script lang="ts">
  // "Add these to the project?": Copy | Move, the names that are taken (listed first, with what to do about them) and
  // what cannot come in. Esc | backdrop | Cancel = nothing happens.
  import type { ConflictChoice, ImportMode, ImportPlan } from "../../api";
  import { t, tn } from "../i18n";
  import Segments from "../shell/settings/Segments.svelte";
  import { canMove, conflictNames, refusalText } from "./importModel";

  type Props = {
    plan: ImportPlan;
    /** The target folder as the person reads it. */
    where: string;
    mode: ImportMode;
    conflicts: ConflictChoice;
    setMode: (mode: ImportMode) => void;
    setConflicts: (conflicts: ConflictChoice) => void;
    confirm: () => void;
    cancel: () => void;
  };
  let { plan, where, mode, conflicts, setMode, setConflicts, confirm, cancel }: Props = $props();

  /** Names listed before "and n more". */
  const SHOWN_NAMES = 6;
  const SHOWN_REFUSED = 4;

  let dialog: HTMLElement | undefined = $state();
  let primary: HTMLButtonElement | undefined = $state();

  const taken = $derived(conflictNames(plan));
  const movable = $derived(canMove(plan));

  const modes = $derived([
    { value: "copy" as const, label: t("import.copy") },
    ...(movable ? [{ value: "move" as const, label: t("import.move") }] : []),
  ]);
  const choices = $derived([
    { value: "keepBoth" as const, label: t("import.keepBoth") },
    { value: "skip" as const, label: t("import.skip") },
  ]);

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
      return;
    }
    if (e.key !== "Tab" || !dialog) return;
    // focus stays inside the dialog
    const focusable = Array.from(
      dialog.querySelectorAll<HTMLElement>("button:not(:disabled):not([tabindex='-1'])"),
    );
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
    if (e.target === e.currentTarget) cancel();
  }}
>
  <div
    bind:this={dialog}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="import-title"
    tabindex="-1"
    {onkeydown}
  >
    <h2 id="import-title">{tn("import.title", plan.items.length, { folder: where })}</h2>

    {#if taken.length > 0}
      <section class="taken" aria-labelledby="import-taken">
        <h3 id="import-taken">{tn("import.taken", taken.length)}</h3>
        <p class="names">
          {taken.slice(0, SHOWN_NAMES).join(" · ")}
          {#if taken.length > SHOWN_NAMES}
            <span class="more">{tn("import.more", taken.length - SHOWN_NAMES)}</span>
          {/if}
        </p>
        <Segments
          options={choices}
          value={conflicts}
          label={t("import.takenLabel")}
          onchange={setConflicts}
        />
        <p class="hint">
          {conflicts === "keepBoth" ? t("import.keepBothHint") : t("import.skipHint")}
        </p>
      </section>
    {/if}

    <section class="how" aria-labelledby="import-how">
      <h3 id="import-how" class="sr-only">{t("import.howLabel")}</h3>
      <Segments options={modes} value={mode} labelledby="import-how" onchange={setMode} />
      <p class="hint">
        {#if !movable}
          {t("import.moveInProject")}
        {:else if mode === "copy"}
          {t("import.copyHint")}
        {:else}
          {t("import.moveHint")}
        {/if}
      </p>
    </section>

    {#if plan.refused.length > 0}
      <section class="refused" aria-labelledby="import-refused">
        <h3 id="import-refused">{tn("import.refused", plan.refused.length)}</h3>
        <ul>
          {#each plan.refused.slice(0, SHOWN_REFUSED) as item (item.source)}
            <li title={item.source}>
              <span class="name">{item.name}</span>
              {refusalText(item.reason)}
            </li>
          {/each}
          {#if plan.refused.length > SHOWN_REFUSED}
            <li>{tn("import.more", plan.refused.length - SHOWN_REFUSED)}</li>
          {/if}
        </ul>
      </section>
    {/if}

    <footer>
      <button type="button" class="btn" onclick={cancel}>{t("common.cancel")}</button>
      <button bind:this={primary} type="button" class="btn btn--primary" onclick={confirm}>
        {mode === "copy" ? t("import.copy") : t("import.move")}
      </button>
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
    width: min(480px, 100%);
    max-height: 100%;
    overflow: auto;
    padding: var(--sp-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  h2 {
    margin: 0 0 var(--sp-4);
    font-size: 15px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  h3 {
    margin: 0 0 var(--sp-1);
    font-size: var(--fs-ui);
    font-weight: 600;
  }
  section {
    margin: 0 0 var(--sp-4);
  }
  .taken {
    padding: var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  .names {
    margin: 0 0 var(--sp-3);
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .more {
    color: var(--faint);
  }
  .hint {
    margin: var(--sp-2) 0 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .refused h3 {
    color: var(--muted);
    font-weight: 500;
  }
  .refused ul {
    margin: 0;
    padding: 0;
    list-style: none;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .refused li {
    overflow-wrap: anywhere;
  }
  .refused .name {
    color: var(--text);
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--sp-2);
    margin-top: var(--sp-5);
  }
</style>
