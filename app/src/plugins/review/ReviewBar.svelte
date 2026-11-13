<script lang="ts">
  // Over the top of the page: "Changed since you last read · 3 changes [Review]"; under the header, in review, its tools:
  // previous | next, Keep all, Revert all, Undo, what to compare with, Done. The keyboard follows the review: opened
  // (from the bar, the palette, a key) it lands on the bar's first control; closed (Done, Esc) it goes back to the
  // page when it had no place left.
  //
  // Against an earlier version, "Revert all" reads "Bring back this version". When that would replace text that is in
  // no commit, the bar asks first, in place of its actions: the keyboard waits on Cancel.
  import { tick } from "svelte";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import type { Basis } from "../../api";
  import { language, t, tn } from "../../core/i18n";
  import { dialogFocus } from "../../core/shell/dialogFocus";
  import { exactTime } from "../../core/time";
  import { review } from "./store.svelte";

  const warningId = $props.id();
  const shown = $derived(review.state);
  /** Why git cannot be asked here (no git, no repository): neither the last commit nor an earlier version can be
   *  chosen; null when it can. */
  const commitWhy = $derived(review.commitUnavailable);
  /** The earlier version the review compares with, or null. */
  const picked = $derived(review.picked);
  /** When that version was made, as readers of the language write it. */
  const pickedWhen = $derived(picked === null ? null : exactTime(picked.atMs, language.locale));
  let bar: HTMLElement | undefined = $state();
  /** The document area of this bar (where the keyboard goes when the review closes). */
  let area: Element | null = null;
  let wasReviewing = false;

  /** The keyboard was on something that went away (the bar's button, a change's Keep): nowhere now. */
  const lost = (): boolean =>
    document.activeElement === null || document.activeElement === document.body;

  $effect(() => {
    const now = shown.status === "reviewing";
    const el = bar;
    if (el) area = el.closest("section.area") ?? area;
    if (now === wasReviewing) return;
    wasReviewing = now;
    // opened: the review's own controls, also when the palette gave the keyboard back to the file list meanwhile
    if (now) el?.querySelector<HTMLElement>("button:not(:disabled)")?.focus();
    else if (lost())
      area?.querySelector<HTMLElement>("[data-scroller]")?.focus({ preventScroll: true });
  });

  let wasConfirming = false;
  $effect(() => {
    const now = review.confirming;
    if (now === wasConfirming) return;
    wasConfirming = now;
    // the question went (answered, or its review was drawn again) with the keyboard on it: the button that asked
    if (!now && lost()) bar?.querySelector<HTMLElement>("[data-revert-all]")?.focus();
  });

  /** "Compare with" is one radio group: one Tab stop (the choice), arrows move the choice like any radio group. */
  let bases: HTMLElement | undefined = $state();
  function onbasiskey(e: KeyboardEvent, at: number): void {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    const step =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    if (step === 0) return;
    e.preventDefault();
    // the choices, then "an earlier version"
    const count = BASES.length + 1;
    const to = (at + step + count) % count;
    const next = BASES[to];
    // a version is picked in its list, which only Enter | Space | a click open: the arrow chooses nothing there
    if (next) void review.setBasis(next.value);
    // the keyboard moves with the arrow, also to a choice that cannot be had here (it says why)
    void tick().then(() => bases?.querySelectorAll<HTMLElement>('[role="radio"]')[to]?.focus());
  }
  const BASES: { value: Basis; label: "review.basis.read" | "review.basis.commit" }[] = [
    { value: "read", label: "review.basis.read" },
    { value: "commit", label: "review.basis.commit" },
  ];

  /** Opens the list of versions, to pick one (or another one). Git cannot be asked: the note says why, as for the
   *  last commit, and nothing opens. */
  function chooseVersion(): void {
    if (commitWhy === null) void review.openVersions();
    else review.basisNote = commitWhy;
  }
</script>

{#if shown.status === "offer"}
  <!-- the offer floats over the top of the page: appearing (a file changed outside) never pushes the text down -->
  <div class="overlay">
    <div class="bar floating" role="region" aria-label={t("review.bar")}>
      <span>{tn("review.offer", shown.count)}</span>
      <button type="button" class="btn btn--sm" onclick={() => void review.begin()}>
        {t("review.review")}
      </button>
    </div>
  </div>
{:else if shown.status === "reviewing"}
  {@const view = shown.view}
  <div class="bar reviewing" role="region" aria-label={t("review.bar")} bind:this={bar}>
    <span class="live" aria-live="polite">{review.announcement}</span>
    <strong>{tn("review.reviewing", view.count)}</strong>
    {#if view.count > 0}
      <span class="position">{t("review.position", { i: shown.at + 1, n: view.count })}</span>
      <button
        type="button"
        class="icon"
        title={t("review.prev")}
        aria-label={t("review.prev")}
        onclick={() => review.go(-1)}><ChevronUp size={15} /></button
      >
      <button
        type="button"
        class="icon"
        title={t("review.next")}
        aria-label={t("review.next")}
        onclick={() => review.go(1)}><ChevronDown size={15} /></button
      >
    {/if}
    <span class="spacer"></span>
    <span class="segments" role="radiogroup" aria-label={t("review.basis.label")} bind:this={bases}>
      {#each BASES as option, at (option.value)}
        {@const why = option.value === "commit" ? commitWhy : null}
        <!-- unavailable: still focusable and says why (title, and a note when chosen), never a silent switch -->
        <button
          type="button"
          role="radio"
          aria-checked={review.basis === option.value}
          tabindex={review.basis === option.value ? 0 : -1}
          aria-disabled={why !== null}
          title={why ?? undefined}
          class={["segment", review.basis === option.value && "is-on", why !== null && "is-off"]}
          onclick={() => void review.setBasis(option.value)}
          onkeydown={(e) => onbasiskey(e, at)}>{t(option.label)}</button
        >
      {/each}
      <!-- an earlier version: chosen in its list; once picked it names the version (cut where the bar is narrow,
           whole in the title) -->
      <button
        type="button"
        role="radio"
        aria-checked={picked !== null}
        tabindex={picked !== null ? 0 : -1}
        aria-disabled={commitWhy !== null}
        title={commitWhy ??
          (pickedWhen === null ? undefined : t("review.comparing", { when: pickedWhen }))}
        class={["segment", "version", picked !== null && "is-on", commitWhy !== null && "is-off"]}
        onclick={chooseVersion}
        onkeydown={(e) => onbasiskey(e, BASES.length)}
        >{pickedWhen === null
          ? t("review.basis.version")
          : t("review.basis.picked", { when: pickedWhen })}</button
      >
    </span>
    {#if review.basisNote !== null}
      <span class="why" role="status">{review.basisNote}</span>
    {/if}
    {#if review.confirming}
      <!-- not a window of its own: the page stays in reach, Tab leaves it; Esc on it is Cancel -->
      <div
        class="confirm"
        role="alertdialog"
        aria-label={t("review.bringBack")}
        aria-describedby={warningId}
        use:dialogFocus={{
          initial: "[data-cancel]",
          onEscape: () => review.cancelBringBack(),
          trapTab: false,
        }}
      >
        <span id={warningId}>{t("review.bringBack.warning")}</span>
        <button
          type="button"
          class="btn btn--sm btn--primary"
          onclick={() => void review.confirmBringBack()}>{t("review.bringBack.confirm")}</button
        >
        <button
          type="button"
          class="btn btn--sm"
          data-cancel
          onclick={() => review.cancelBringBack()}>{t("common.cancel")}</button
        >
      </div>
    {:else}
      {#if view.canKeep}
        <button
          type="button"
          class="btn btn--sm"
          title={t("review.keepAllHint")}
          onclick={() => void review.keepAll()}>{t("review.keepAll")}</button
        >
      {/if}
      <button
        type="button"
        class="btn btn--sm"
        data-revert-all
        title={picked === null ? t("review.revertAllHint") : undefined}
        onclick={() => void review.revertAll()}
        >{t(picked === null ? "review.revertAll" : "review.bringBack")}</button
      >
      <button
        type="button"
        class="btn btn--sm"
        disabled={review.undoable === 0}
        onclick={() => void review.undo()}
      >
        <Undo2 size={13} aria-hidden="true" />
        {t("review.undo")}
      </button>
      <button
        type="button"
        class="btn btn--sm"
        title={t("review.doneHint")}
        onclick={() => void review.done()}>{t("review.done")}</button
      >
    {/if}
  </div>
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-4);
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--accent) 7%, var(--bg));
    font-size: var(--fs-ui-sm);
  }
  .overlay {
    position: relative;
    z-index: 6;
    height: 0;
  }
  .floating {
    position: absolute;
    top: var(--sp-2);
    right: var(--sp-4);
    max-width: calc(100% - 2 * var(--sp-4));
    padding: var(--sp-1) var(--sp-2) var(--sp-1) var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    box-shadow: var(--shadow-pop);
  }
  .spacer {
    flex: 1;
  }
  .position {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .live {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .icon {
    display: inline-grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--hover);
    color: var(--text);
  }
  .segments {
    display: inline-flex;
    /* never wider than the bar: the version takes a line of its own, and its label is cut when that is too long */
    flex-wrap: wrap;
    min-width: 0;
    max-width: 100%;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  .segment {
    flex: none;
    padding: 2px 8px;
    border: 0;
    border-radius: calc(var(--radius-control) - 2px);
    background: transparent;
    color: var(--muted);
    font: inherit;
    white-space: nowrap;
    cursor: pointer;
  }
  .segment.version {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* the question takes a line of its own under the bar's first one; its sentence wraps, the answers stay whole */
  .confirm {
    display: flex;
    flex: 1 1 100%;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
  .confirm > span {
    flex: 1 1 24em;
    min-width: 0;
  }
  .why {
    color: var(--muted);
  }
  .segment.is-off {
    cursor: default;
    opacity: 0.6;
  }
  .segment.is-on {
    background: var(--bg);
    color: var(--text);
    font-weight: 600;
  }
</style>
