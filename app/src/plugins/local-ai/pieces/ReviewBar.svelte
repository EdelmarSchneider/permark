<script module lang="ts">
  import type { MessageKey, PluralKey } from "../../../core/i18n";
  import type { Found } from "./review.svelte";

  /** The bar's words, each plugin its own: message keys (`{…}` = what the bar fills in), read when drawn. */
  export type ReviewLabels = {
    /** The bar's name. */
    bar: MessageKey;
    /** The scope held nothing to send. */
    nothing: MessageKey;
    /** Getting the model ready. */
    loading: MessageKey;
    /** Part `{i}` of `{n}` is with the model. */
    checking: MessageKey;
    /** The run ended with nothing to suggest. */
    clean: MessageKey;
    /** The run ended with `{n}` suggestions. */
    found: PluralKey;
    /** The run was stopped with `{n}` suggestions so far. */
    stopped: PluralKey;
    /** The run failed. */
    failed: MessageKey;
    /** Why: no model on the plugin's job (fixed on the Local AI page). */
    noModel: MessageKey;
    /** Suggestion `{i}` of `{n}`, in figures. */
    position: MessageKey;
    prev: MessageKey;
    next: MessageKey;
    /** The excerpt's name: its suggestion shown in the text. */
    show: MessageKey;
    /** `{n}` parts left as they were. */
    discarded: PluralKey;
    /** `{n}` parts whose answer said the same as the part (none: a plugin whose review never counts them; no note).
     *  A function: the plugin's own words for the count, where they hold more than it (a language's name). */
    same?: PluralKey | ((n: number) => string);
    /** `{n}` suggestions the text no longer matches. */
    outdated: PluralKey;
    retry: MessageKey;
    openSettings: MessageKey;
    accept: MessageKey;
    reject: MessageKey;
    acceptAll: MessageKey;
    rejectAll: MessageKey;
    done: MessageKey;
    /** What a screen reader hears for suggestion `at` (from 0) of `n`. */
    spoken: (suggestion: Found, at: number, n: number) => string;
  };
</script>

<script lang="ts">
  // The strip under the header while a plugin's piece run goes on | is reviewed (nothing before a run); the words and
  // what the excerpt draws are the plugin's. Row 1: where the run stands (getting the AI engine the first time + its
  // size progress, getting the model ready + progress, checking part i of n, the count) + "3 / 12" + previous | next;
  // row 2: the current suggestion as the plugin draws it, notes (parts not sent because the editor on screen cannot
  // change them, parts left as they were, parts whose answer said the same when the plugin has words for it,
  // suggestions the text no longer matches), a failure with Try again + Local AI settings; row 3: Accept | Reject, all
  // of them, Done, Cancel. A run that ended with none left: the plugin's "none found" only when it found none, else
  // "all reviewed". Nothing sent because the editor cannot change any part of the scope: that note, not the plugin's
  // "nothing here".
  // The excerpt = a button: click | Enter -> the suggestion selected in the text (an excerpt with controls of its
  // own stands without it: `excerptAs`). One polite live region: the phase, then the count + the current
  // suggestion. A focused button turned off -> the keyboard to Cancel (running) | Done.
  // Keys inside the bar: A | R, Shift+A | Shift+R, Esc (Done, back to the editor). The bar carries its plugin's name
  // (`data-<plugin>`: correction's = `data-correction`).
  import { untrack, type Snippet } from "svelte";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import { isTypingTarget } from "../../../core/commands/dispatch";
  import { sizeText, t, tn } from "../../../core/i18n";
  import { ui } from "../../../core/shell/ui.svelte";
  import { LOCAL_AI_PAGE } from "../store.svelte";
  import { percentText, runErrorText } from "../words";
  import { running, type PieceReview, type Review } from "./review.svelte";

  type Props = {
    /** The review shown and acted on (one per bar). */
    review: PieceReview;
    labels: ReviewLabels;
    /** Draws a suggestion inside the excerpt. */
    excerpt: Snippet<[Found]>;
    /** How the excerpt stands in the bar. `button` (default): inside one button that shows the suggestion in the
     *  text, cut to three lines, named by `labels.show`. `own`: as the plugin draws it, nothing around it: its sizes,
     *  its names and its way to the suggestion in the text (`review.show`) are the plugin's. */
    excerptAs?: "button" | "own";
  };
  let { review: store, labels, excerpt, excerptAs = "button" }: Props = $props();

  const shown = $derived(store.state);
  const review = $derived(shown.kind === "review" ? shown : null);
  const busy = $derived(review !== null && running(review.status));
  const count = $derived(review?.suggestions.length ?? 0);
  const current = $derived(review?.suggestions[review.at] ?? null);
  /** What the live region says: the phase (not each part checked; a failure speaks as an alert), the current step. */
  const announced = $derived.by(() => {
    if (review === null) return "";
    const quiet = review.status.kind === "checking" || review.status.kind === "failed";
    const phase = quiet ? "" : headline(review);
    const step = current === null ? "" : labels.spoken(current, review.at, count);
    return [phase, step].filter((part) => part !== "").join(" ");
  });

  let bar: HTMLElement | undefined = $state();
  /** The bar's button that has the keyboard (null: the keyboard went elsewhere). */
  let focused: HTMLButtonElement | null = null;

  function onfocusin(event: FocusEvent): void {
    focused = event.target instanceof HTMLButtonElement ? event.target : null;
  }

  function onfocusout(event: FocusEvent): void {
    const next = event.relatedTarget;
    if (next instanceof Node && !bar?.contains(next)) focused = null;
  }

  // the focused button turned off | went away (Accept with no suggestion, Cancel at the run's end): the keyboard
  // stays in the bar, on Cancel while running, else Done
  $effect(() => {
    void [current, busy, count];
    const el = bar;
    const was = focused;
    if (!el || was === null || (el.contains(was) && !was.disabled)) return;
    const active = document.activeElement;
    if (active !== was && active !== document.body && active !== null) return;
    const to =
      el.querySelector<HTMLElement>('[data-action="cancel"]') ??
      el.querySelector<HTMLElement>('[data-action="done"]');
    to?.focus();
  });

  // the plugin's key while reviewing (correction: Mod+Alt+K): the keyboard to Accept (none: the bar's first button);
  // what was asked before this bar was there does not count
  let asked = untrack(() => store.focusRequest);
  $effect(() => {
    const request = store.focusRequest;
    const el = bar;
    if (request === asked || !el) return;
    asked = request;
    const accept = el.querySelector<HTMLElement>('[data-action="accept"]:not(:disabled)');
    (accept ?? el.querySelector<HTMLElement>("button:not(:disabled)"))?.focus();
  });

  /** Where the run stands; at its end: how many suggestions are left, or why none is. */
  function headline({ status, suggestions, total, discarded }: Review): string {
    const left = suggestions.length;
    switch (status.kind) {
      case "engine":
        return t("localAi.test.engine");
      case "loading":
        return t(labels.loading);
      case "checking":
        return t(labels.checking, { i: status.part, n: status.parts });
      case "finished":
        if (left > 0) return tn(labels.found, left);
        // none left: every one found is handled | none was found (parts left as they were say so in their note)
        if (total > 0) return t("localAi.review.allReviewed");
        return discarded === 0 ? t(labels.clean) : tn(labels.found, 0);
      case "cancelled":
        return tn(labels.stopped, left);
      case "failed":
        return t(labels.failed);
      default:
        return status satisfies never;
    }
  }

  /** What the bar says of `n` answers that said the same as their part. */
  function sameNote(same: NonNullable<ReviewLabels["same"]>, n: number): string {
    return typeof same === "function" ? same(n) : tn(same, n);
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return;
    if (isTypingTarget(event.target)) return;
    const key = event.key.toLowerCase();
    let act: (() => void) | null = null;
    if (event.key === "Escape") act = () => store.done();
    else if (key === "a") act = event.shiftKey ? () => store.acceptAll() : () => store.accept();
    else if (key === "r") act = event.shiftKey ? () => store.rejectAll() : () => store.reject();
    if (act === null) return;
    event.preventDefault();
    event.stopPropagation();
    act();
  }
</script>

{#if shown.kind !== "closed"}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="bar"
    role="group"
    aria-label={t(labels.bar)}
    {...{ [`data-${store.plugin}`]: "" }}
    bind:this={bar}
    {onkeydown}
    {onfocusin}
    {onfocusout}
  >
    {#if shown.kind === "nothing"}
      <div class="row">
        <span class="status" role="status" aria-live="polite"
          >{shown.locked > 0 ? tn("localAi.review.locked", shown.locked) : t(labels.nothing)}</span
        >
        <span class="spacer"></span>
        <button type="button" class="btn btn--sm" onclick={() => store.done()}>
          {t(labels.done)}
        </button>
      </div>
    {:else if review !== null}
      <div class="row">
        <p class="sr-only" role="status" aria-live="polite">{announced}</p>
        <span class="status">{headline(review)}</span>
        {#if review.status.kind === "engine"}
          {@const { receivedBytes, totalBytes } = review.status}
          <progress
            max={totalBytes > 0 ? totalBytes : 1}
            value={receivedBytes}
            aria-label={t("localAi.test.engine")}
          ></progress>
          <span class="quiet"
            >{t("localAi.dl.progress", {
              done: sizeText(receivedBytes),
              total: sizeText(totalBytes),
            })}</span
          >
        {:else if review.status.kind === "loading"}
          <progress max={1} value={review.status.progress} aria-label={t(labels.loading)}
          ></progress>
          <span class="quiet">{percentText(review.status.progress, 1)}</span>
        {/if}
        {#if count > 0}
          <span class="position" aria-hidden="true"
            >{t(labels.position, { i: review.at + 1, n: count })}</span
          >
          <button
            type="button"
            class="icon"
            title={t(labels.prev)}
            aria-label={t(labels.prev)}
            onclick={() => store.go(-1)}><ChevronUp size={15} /></button
          >
          <button
            type="button"
            class="icon"
            title={t(labels.next)}
            aria-label={t(labels.next)}
            onclick={() => store.go(1)}><ChevronDown size={15} /></button
          >
        {/if}
      </div>
      {#if current !== null && excerptAs === "own"}
        {@render excerpt(current)}
      {:else if current !== null}
        <button
          type="button"
          class="excerpt"
          data-action="show"
          title={t(labels.show)}
          aria-label={t(labels.show)}
          onclick={() => store.show()}
        >
          <span aria-hidden="true">{@render excerpt(current)}</span>
        </button>
      {/if}
      {#if review.locked > 0}
        <p class="quiet">{tn("localAi.review.locked", review.locked)}</p>
      {/if}
      {#if review.discarded > 0}
        <p class="quiet">{tn(labels.discarded, review.discarded)}</p>
      {/if}
      {#if labels.same !== undefined && review.same > 0}
        <p class="quiet">{sameNote(labels.same, review.same)}</p>
      {/if}
      {#if review.outdated > 0}
        <p class="quiet">{tn(labels.outdated, review.outdated)}</p>
      {/if}
      {#if review.status.kind === "failed"}
        <div class="problem" role="alert">
          <span>{runErrorText(review.status.error, labels.noModel)}</span>
          <button type="button" class="btn btn--sm" onclick={() => void store.retry()}>
            {t(labels.retry)}
          </button>
          <button
            type="button"
            class="btn btn--sm"
            onclick={() => ui.openSettingsPage(LOCAL_AI_PAGE, "content")}
          >
            {t(labels.openSettings)}
          </button>
        </div>
      {/if}
      <div class="row">
        <button
          type="button"
          class="btn btn--sm"
          data-action="accept"
          disabled={current === null}
          onclick={() => store.accept()}>{t(labels.accept)}</button
        >
        <button
          type="button"
          class="btn btn--sm"
          data-action="reject"
          disabled={current === null}
          onclick={() => store.reject()}>{t(labels.reject)}</button
        >
        <button
          type="button"
          class="btn btn--sm"
          data-action="acceptAll"
          disabled={busy || count === 0}
          onclick={() => store.acceptAll()}>{t(labels.acceptAll)}</button
        >
        <button
          type="button"
          class="btn btn--sm"
          data-action="rejectAll"
          disabled={busy || count === 0}
          onclick={() => store.rejectAll()}>{t(labels.rejectAll)}</button
        >
        <span class="spacer"></span>
        {#if busy}
          <button
            type="button"
            class="btn btn--sm btn--ghost"
            data-action="cancel"
            onclick={() => store.cancel()}>{t("common.cancel")}</button
          >
        {/if}
        <button type="button" class="btn btn--sm" data-action="done" onclick={() => store.done()}
          >{t(labels.done)}</button
        >
      </div>
    {/if}
  </div>
{/if}

<style>
  .bar {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-2) var(--sp-4);
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--accent) 7%, var(--bg));
    font-size: var(--fs-ui-sm);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
  }
  .spacer {
    flex: 1;
  }
  .status {
    font-weight: 600;
  }
  .position,
  .quiet {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  p {
    margin: 0;
  }
  progress {
    width: 8rem;
    accent-color: var(--accent);
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
  .excerpt {
    display: -webkit-box;
    margin: 0 calc(-1 * var(--sp-1));
    padding: 0 var(--sp-1);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    font: inherit;
    text-align: start;
    cursor: pointer;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    color: var(--text);
    font-size: var(--fs-ui);
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  .excerpt:hover {
    background: var(--hover);
  }
  .problem {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
  }
</style>
