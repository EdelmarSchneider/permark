<script lang="ts">
  // The question at the drop point on the Explorer: what comes in, the names that exist here (Keep both | Skip), what
  // cannot come in, then Copy here | Move here | Cancel. While the plan is on its way: "Checking names…", buttons off.
  import type { ConflictChoice, ImportMode, ImportPlan } from "../../api";
  import { t, tn } from "../i18n";
  import DropPopover from "../shell/DropPopover.svelte";
  import Segments from "../shell/settings/Segments.svelte";
  import { canMove, conflictNames, countsText, firstOf, refusalText } from "./importModel";

  type Props = {
    at: { x: number; y: number };
    /** The target folder as the person reads it. */
    where: string;
    /** `null` while the plan is on its way. */
    plan: ImportPlan | null;
    conflicts: ConflictChoice;
    setConflicts: (conflicts: ConflictChoice) => void;
    confirm: (mode: ImportMode) => void;
    cancel: () => void;
  };
  let { at, where, plan, conflicts, setConflicts, confirm, cancel }: Props = $props();

  /** Names listed before "and n more". */
  const SHOWN_NAMES = 4;
  const SHOWN_REFUSED = 3;

  const taken = $derived(plan === null ? [] : conflictNames(plan));
  const movable = $derived(plan !== null && canMove(plan));
  const nothing = $derived(plan !== null && plan.items.length === 0);
  const choices = $derived([
    { value: "keepBoth" as const, label: t("import.keepBoth") },
    { value: "skip" as const, label: t("import.skip") },
  ]);
</script>

<DropPopover
  {at}
  labelledby="import-ask-title"
  describedby="import-ask-counts"
  close={cancel}
  ready={plan !== null}
>
  <h2 id="import-ask-title">{t("import.ask.title", { folder: where })}</h2>

  {#if plan === null}
    <p id="import-ask-counts" class="line muted" role="status">{t("import.ask.checking")}</p>
  {:else if nothing}
    <p id="import-ask-counts" class="line">{t("import.ask.nothing")}</p>
  {:else}
    <p id="import-ask-counts" class="line">{countsText(plan)}</p>
  {/if}

  {#if taken.length > 0}
    <section class="block" aria-labelledby="import-ask-taken">
      <p id="import-ask-taken" class="line">{tn("import.taken", taken.length)}</p>
      <p class="line names">{firstOf(taken, SHOWN_NAMES)}</p>
      <p class="line">{t("import.takenLabel")}:</p>
      <Segments
        options={choices}
        value={conflicts}
        label={t("import.takenLabel")}
        onchange={setConflicts}
      />
      <p class="line muted hint">
        {conflicts === "keepBoth" ? t("import.keepBothHint") : t("import.skipHint")}
      </p>
    </section>
  {/if}

  {#if plan !== null && plan.refused.length > 0}
    <section class="block muted" aria-labelledby="import-ask-refused">
      <p id="import-ask-refused" class="line">{tn("import.refused", plan.refused.length)}:</p>
      <ul>
        {#each plan.refused.slice(0, SHOWN_REFUSED) as item (item.source)}
          <li title={item.source}>{item.name} {refusalText(item.reason)}</li>
        {/each}
        {#if plan.refused.length > SHOWN_REFUSED}
          <li>{tn("import.more", plan.refused.length - SHOWN_REFUSED)}</li>
        {/if}
      </ul>
    </section>
  {/if}

  {#if !nothing}
    <p class="line muted hint">
      {plan !== null && !movable ? t("import.moveInProject") : t("import.ask.hint")}
    </p>
  {/if}

  <footer>
    {#if nothing}
      <button type="button" class="btn btn--primary" data-autofocus onclick={cancel}>
        {t("common.close")}
      </button>
    {:else}
      <button
        type="button"
        class="btn btn--primary"
        data-autofocus
        disabled={plan === null}
        onclick={() => confirm("copy")}
      >
        {t("import.copyHere")}
      </button>
      {#if plan === null || movable}
        <button type="button" class="btn" disabled={plan === null} onclick={() => confirm("move")}>
          {t("import.moveHere")}
        </button>
      {/if}
      <button type="button" class="btn" onclick={cancel}>{t("common.cancel")}</button>
    {/if}
  </footer>
</DropPopover>

<style>
  h2 {
    margin: 0 0 var(--sp-2);
    font-size: var(--fs-ui);
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .line {
    margin: 0 0 var(--sp-1);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--muted);
  }
  .names {
    color: var(--text);
  }
  .hint {
    margin-top: var(--sp-2);
  }
  .block {
    margin-top: var(--sp-3);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--fs-ui-sm);
  }
  li {
    overflow-wrap: anywhere;
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--sp-2);
    margin-top: var(--sp-3);
  }
</style>
