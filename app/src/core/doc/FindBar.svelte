<script lang="ts">
  // Find in page: a small bar at the top right of the document. Enter goes to the next match, Shift+Enter to the
  // previous, Escape closes. The matches are painted without touching the document.
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import X from "@lucide/svelte/icons/x";
  import { t } from "../i18n";
  import { documents } from "./document.svelte";
  import { FIND_LIMIT, clearPaint, findIn, paint } from "./find";

  type Props = { root: HTMLElement; close: () => void };
  let { root, close }: Props = $props();

  let query = $state("");
  let input: HTMLInputElement | undefined = $state();
  let ranges = $state.raw<readonly Range[]>([]);
  let current = $state(0);
  let capped = $state(false);

  /** Looks again in the page; keeps the match number when there still is one. */
  function search(keep: boolean): void {
    const found = query !== "" ? findIn(root, query) : { ranges: [], capped: false };
    ranges = found.ranges;
    capped = found.capped;
    current = keep ? Math.min(current, Math.max(0, ranges.length - 1)) : 0;
    paint(ranges, current);
    if (!keep) reveal();
  }

  function reveal(): void {
    const range = ranges[current];
    if (!range) return;
    const box = range.getBoundingClientRect();
    const pane = root.getBoundingClientRect();
    if (box.top < pane.top + 40 || box.bottom > pane.bottom - 40) {
      root.scrollTop += box.top - pane.top - pane.height / 3;
    }
  }

  function step(direction: 1 | -1): void {
    if (ranges.length === 0) return;
    current = (current + direction + ranges.length) % ranges.length;
    paint(ranges, current);
    reveal();
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      step(event.shiftKey ? -1 : 1);
    } else if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  }

  // focus on open; the paint is undone when the bar goes; a changed page is searched again
  $effect(() => {
    input?.focus();
    input?.select();
    const stop = documents.onPainted(() => search(true));
    return () => {
      stop();
      clearPaint();
    };
  });

  const count = $derived(
    query === ""
      ? ""
      : ranges.length === 0
        ? t("find.none")
        : t("find.count", {
            index: current + 1,
            total: capped ? `${FIND_LIMIT}+` : ranges.length,
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
    disabled={ranges.length === 0}
    onclick={() => step(-1)}
  >
    <ChevronUp size={15} />
  </button>
  <button
    type="button"
    class="icon-btn icon-btn--sm"
    title={t("find.nextHint")}
    aria-label={t("find.next")}
    disabled={ranges.length === 0}
    onclick={() => step(1)}
  >
    <ChevronDown size={15} />
  </button>
  <button
    type="button"
    class="icon-btn icon-btn--sm"
    title={t("find.closeHint")}
    aria-label={t("find.close")}
    onclick={close}
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
