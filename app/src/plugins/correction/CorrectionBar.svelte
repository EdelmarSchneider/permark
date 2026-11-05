<script lang="ts">
  // The strip under the header while a correction runs | is reviewed (nothing before a run). Row 1: where the run
  // stands (getting the AI engine the first time + its size progress, getting the model ready + progress, checking
  // part i of n, the count) + "3 / 12" + previous | next; row 2:
  // the current suggestion in its text (removed | added words marked), notes (parts left as they were, suggestions the
  // text no longer matches), a failure with Try again + Local AI settings; row 3: Accept | Reject, all of them, Done,
  // Cancel. The excerpt = the store's cut (its block, word edges, `…`). One polite live region: the phase, then the count
  // + the current suggestion. A focused button turned off -> the keyboard to Cancel (running) | Done. Keys inside the
  // bar: A | R, Shift+A | Shift+R, Esc (Done, back to the editor).
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import { isTypingTarget } from "../../core/commands/dispatch";
  import { sizeText, t, tn } from "../../core/i18n";
  import { ui } from "../../core/shell/ui.svelte";
  import { LOCAL_AI_PAGE } from "../local-ai/store.svelte";
  import { percentText } from "../local-ai/words";
  import { correction, running, type Found, type RunStatus } from "./store.svelte";
  import { failureText } from "./words";

  const shown = $derived(correction.state);
  const review = $derived(shown.kind === "review" ? shown : null);
  const busy = $derived(review !== null && running(review.status));
  const count = $derived(review?.suggestions.length ?? 0);
  const current = $derived(review?.suggestions[review.at] ?? null);
  /** What the live region says: the phase (not each part checked; a failure speaks as an alert), the current step. */
  const announced = $derived.by(() => {
    if (review === null) return "";
    const quiet = review.status.kind === "checking" || review.status.kind === "failed";
    const phase = quiet ? "" : headline(review.status, count, review.discarded);
    const step = current === null ? "" : spoken(current, review.at, count);
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

  // Mod+Alt+K while reviewing: the keyboard to Accept (none: the bar's first button)
  let asked = correction.focusRequest;
  $effect(() => {
    const request = correction.focusRequest;
    const el = bar;
    if (request === asked || !el) return;
    asked = request;
    const accept = el.querySelector<HTMLElement>('[data-action="accept"]:not(:disabled)');
    (accept ?? el.querySelector<HTMLElement>("button:not(:disabled)"))?.focus();
  });

  function headline(status: RunStatus, found: number, discarded: number): string {
    switch (status.kind) {
      case "engine":
        return t("localAi.test.engine");
      case "loading":
        return t("correction.loading");
      case "checking":
        return t("correction.checking", { i: status.part, n: status.parts });
      case "finished":
        return found === 0 && discarded === 0
          ? t("correction.clean")
          : tn("correction.found", found);
      case "cancelled":
        return tn("correction.stopped", found);
      case "failed":
        return t("correction.failed");
      default:
        return status satisfies never;
    }
  }

  /** What a screen reader hears for the current suggestion. */
  function spoken(s: Found, at: number, n: number): string {
    const params = { i: at + 1, n, a: s.original, b: s.insert };
    if (s.original === "") return t("correction.sr.insert", params);
    if (s.insert === "") return t("correction.sr.remove", params);
    return t("correction.sr.replace", params);
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return;
    if (isTypingTarget(event.target)) return;
    const key = event.key.toLowerCase();
    let act: (() => void) | null = null;
    if (event.key === "Escape") act = () => correction.done();
    else if (key === "a")
      act = event.shiftKey ? () => correction.acceptAll() : () => correction.accept();
    else if (key === "r")
      act = event.shiftKey ? () => correction.rejectAll() : () => correction.reject();
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
    aria-label={t("correction.bar")}
    data-correction
    bind:this={bar}
    {onkeydown}
    {onfocusin}
    {onfocusout}
  >
    {#if review === null}
      <div class="row">
        <span class="status" role="status" aria-live="polite">{t("correction.nothing")}</span>
        <span class="spacer"></span>
        <button type="button" class="btn btn--sm" onclick={() => correction.done()}>
          {t("correction.done")}
        </button>
      </div>
    {:else}
      <div class="row">
        <p class="sr-only" role="status" aria-live="polite">{announced}</p>
        <span class="status">{headline(review.status, count, review.discarded)}</span>
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
          <progress max={1} value={review.status.progress} aria-label={t("correction.loading")}
          ></progress>
          <span class="quiet">{percentText(review.status.progress, 1)}</span>
        {/if}
        {#if count > 0}
          <span class="position" aria-hidden="true"
            >{t("correction.position", { i: review.at + 1, n: count })}</span
          >
          <button
            type="button"
            class="icon"
            title={t("correction.prev")}
            aria-label={t("correction.prev")}
            onclick={() => correction.go(-1)}><ChevronUp size={15} /></button
          >
          <button
            type="button"
            class="icon"
            title={t("correction.next")}
            aria-label={t("correction.next")}
            onclick={() => correction.go(1)}><ChevronDown size={15} /></button
          >
        {/if}
      </div>
      {#if current !== null}
        <p class="excerpt">
          <span aria-hidden="true"
            >{current.before}<del>{current.original}</del><ins>{current.insert}</ins
            >{current.after}</span
          >
        </p>
      {/if}
      {#if review.discarded > 0}
        <p class="quiet">{tn("correction.discarded", review.discarded)}</p>
      {/if}
      {#if review.outdated > 0}
        <p class="quiet">{tn("correction.outdated", review.outdated)}</p>
      {/if}
      {#if review.status.kind === "failed"}
        <div class="problem" role="alert">
          <span>{failureText(review.status.error)}</span>
          <button type="button" class="btn btn--sm" onclick={() => void correction.retry()}>
            {t("correction.retry")}
          </button>
          <button
            type="button"
            class="btn btn--sm"
            onclick={() => ui.openSettingsPage(LOCAL_AI_PAGE, "content")}
          >
            {t("correction.openSettings")}
          </button>
        </div>
      {/if}
      <div class="row">
        <button
          type="button"
          class="btn btn--sm"
          data-action="accept"
          disabled={current === null}
          onclick={() => correction.accept()}>{t("correction.accept")}</button
        >
        <button
          type="button"
          class="btn btn--sm"
          data-action="reject"
          disabled={current === null}
          onclick={() => correction.reject()}>{t("correction.reject")}</button
        >
        <button
          type="button"
          class="btn btn--sm"
          data-action="acceptAll"
          disabled={busy || count === 0}
          onclick={() => correction.acceptAll()}>{t("correction.acceptAll")}</button
        >
        <button
          type="button"
          class="btn btn--sm"
          data-action="rejectAll"
          disabled={busy || count === 0}
          onclick={() => correction.rejectAll()}>{t("correction.rejectAll")}</button
        >
        <span class="spacer"></span>
        {#if busy}
          <button
            type="button"
            class="btn btn--sm btn--ghost"
            data-action="cancel"
            onclick={() => correction.cancel()}>{t("common.cancel")}</button
          >
        {/if}
        <button
          type="button"
          class="btn btn--sm"
          data-action="done"
          onclick={() => correction.done()}>{t("correction.done")}</button
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
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    color: var(--text);
    font-size: var(--fs-ui);
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  del {
    background: var(--review-del-bg);
    color: var(--git-deleted-text);
  }
  ins {
    background: var(--review-add-bg);
    color: var(--git-added-text);
  }
  .problem {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
  }
</style>
