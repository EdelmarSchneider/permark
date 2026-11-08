<script lang="ts">
  // Find in page: a small bar at the top right of the document. Enter | F3 go to the next match, with Shift to the
  // previous, Escape closes. The matches are painted without touching the document. What is searched is a `Finder`:
  // the page in View, the text in Edit; the bar closes when its finder goes (another mode, another editor).
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import X from "@lucide/svelte/icons/x";
  import { untrack } from "svelte";
  import { t } from "../i18n";
  import { FIND_LIMIT, type Finder } from "./find";

  type Props = { finder: Finder; close: () => void };
  let { finder, close }: Props = $props();

  let query = $state("");
  let input: HTMLInputElement | undefined = $state();
  let total = $state(0);
  let current = $state(0);
  let capped = $state(false);

  /** Looks again; keeps the match number when there still is one. */
  function search(keep: boolean): void {
    const found = finder.search(query);
    total = found.total;
    capped = found.capped;
    current = keep ? Math.min(current, Math.max(0, total - 1)) : 0;
    finder.show(current, !keep);
  }

  function step(direction: 1 | -1): void {
    if (total === 0) return;
    current = (current + direction + total) % total;
    finder.show(current, true);
  }

  /** Closes the bar; the finder hands the current match on (Edit: selected, the editor takes the keyboard). */
  function finish(): void {
    finder.done?.(total > 0 ? current : null);
    close();
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.isComposing) return;
    if (event.key === "Enter" || event.key === "F3") {
      event.preventDefault();
      step(event.shiftKey ? -1 : 1);
    } else if (event.key === "Escape") {
      event.preventDefault();
      finish();
    }
  }

  // focus on open
  $effect(() => {
    input?.focus();
    input?.select();
  });

  // a changed page | text is searched again; the paint goes with the finder, and so does the bar
  $effect(() => {
    const target = finder;
    const stop = target.watch(() => search(true));
    return () => {
      stop();
      target.clear();
      untrack(() => close());
    };
  });

  const count = $derived(
    query === ""
      ? ""
      : total === 0
        ? t("find.none")
        : t("find.count", {
            index: current + 1,
            total: capped ? `${FIND_LIMIT}+` : total,
          }),
  );
</script>

<div class="find" role="search" aria-label={t("find.label")}>
  <input
    bind:this={input}
    bind:value={query}
    type="text"
    class="field"
    placeholder={t("find.placeholder")}
    aria-label={t("find.label")}
    spellcheck="false"
    autocomplete="off"
    oninput={() => search(false)}
    {onkeydown}
  />
  <span class="count" aria-live="polite">{count}</span>
  <button
    type="button"
    class="icon-btn icon-btn--sm"
    title={t("find.previousHint")}
    aria-label={t("find.previous")}
    disabled={total === 0}
    onclick={() => step(-1)}
  >
    <ChevronUp size={15} />
  </button>
  <button
    type="button"
    class="icon-btn icon-btn--sm"
    title={t("find.nextHint")}
    aria-label={t("find.next")}
    disabled={total === 0}
    onclick={() => step(1)}
  >
    <ChevronDown size={15} />
  </button>
  <button
    type="button"
    class="icon-btn icon-btn--sm"
    title={t("find.closeHint")}
    aria-label={t("find.close")}
    onclick={finish}
  >
    <X size={15} />
  </button>
</div>

<style>
  .find {
    position: absolute;
    top: var(--sp-2);
    right: var(--sp-4);
    z-index: 20;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
    animation: pm-fade var(--dur-fast) var(--ease);
  }
  .field {
    width: 190px;
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font-size: var(--fs-ui);
  }
  .field:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 0;
  }
  .count {
    min-width: 5.5em;
    padding: 0 6px;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    text-align: center;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
</style>
