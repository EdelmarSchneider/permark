<script lang="ts">
  // The quiet bar under the document header (docs/design/review-changes-ui.md). One slim line when a document
  // changed since it was last read; the review itself replaces it while it is open.
  import { t, tn } from "../i18n";
  import { docs } from "../stores/docs.svelte";
  import { edit } from "../stores/edit.svelte";
  import { review } from "../stores/review.svelte";
  import { vault } from "../stores/vault.svelte";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";

  let width = $state(800);
  let more = $state(false);
  const narrow = $derived(width < 560);
  const tiny = $derived(width < 400);
  const view = $derived(review.view);
  const here = $derived(
    view !== null && view.path === docs.path && docs.current?.kind === "markdown" && !edit.active && !vault.isSimple && !vault.isNotes,
  );
  const changes = $derived(here && view?.status === "changes" ? view.changes.length : 0);
  const offer = $derived(
    here &&
      view !== null &&
      !review.open &&
      ((view.status === "changes" && review.dismissedHash !== view.hash) ||
        (review.mode === "gate" && (view.status === "new" || view.status === "deleted"))),
  );
  const flash = $derived(review.doneUntil > Date.now());

  $effect(() => {
    const until = review.doneUntil;
    if (until === 0) return;
    const timer = setTimeout(() => {
      if (review.doneUntil === until) review.doneUntil = 0;
    }, Math.max(0, until - Date.now()));
    return () => clearTimeout(timer);
  });
</script>

{#if review.open && here && changes > 0}
  <div
    class="banner review-bar"
    role="toolbar"
    aria-label={t("review.bar")}
    bind:clientWidth={width}
  >
    {#if !narrow}
      <span>{tn("review.reviewing", changes)}</span>
    {/if}
    <span class="review-bar__nav">
      <IconButton label={t("review.prev")} onclick={() => review.move(-1)} icon="chevrons-left" />
      <span class="review-bar__pos">{t("review.position", { i: review.current + 1, n: changes })}</span>
      <IconButton label={t("review.next")} onclick={() => review.move(1)} icon="chevrons-right" />
    </span>
    <span class="spacer"></span>
    {#if narrow}
      <button type="button" class="btn btn--ghost btn--sm" aria-expanded={more} onclick={() => (more = !more)}>
        {t("review.more")}
      </button>
      {#if more}
        <button type="button" class="btn btn--ghost btn--sm" title={t("review.keepAllHint")} onclick={() => void review.keepAll()}>
          {t("review.keepAll")}
        </button>
        <button type="button" class="btn btn--ghost btn--sm" title={t("review.revertAllHint")} onclick={() => void review.revertAll()}>
          {t("review.revertAll")}
        </button>
      {/if}
    {:else}
      <button type="button" class="btn btn--ghost btn--sm" title={t("review.keepAllHint")} onclick={() => void review.keepAll()}>
        {t("review.keepAll")}
      </button>
      <button type="button" class="btn btn--ghost btn--sm" title={t("review.revertAllHint")} onclick={() => void review.revertAll()}>
        {t("review.revertAll")}
      </button>
    {/if}
    <button type="button" class="btn btn--ghost btn--sm" title={t("review.doneHint")} onclick={() => review.closeLayer()}>
      {#if tiny}<Icon name="x" size="sm" />{:else}{t("review.done")}{/if}
    </button>
    <span class="sr-only" aria-live="polite">{review.announcement}</span>
  </div>
{:else if offer && view}
  <div class="banner banner--quiet review-bar" role="status" bind:clientWidth={width}>
    <span>
      {#if review.mode === "gate"}
        {tn("review.gate", view.status === "changes" ? changes : 1)}
      {:else if tiny}
        {tn("review.offerShort", changes)}
      {:else}
        {tn("review.offer", changes)}
      {/if}
    </span>
    <span class="spacer"></span>
    <button type="button" class="btn btn--ghost btn--sm review-bar__go" onclick={() => review.start()}>
      {t("review.review")}
    </button>
    {#if review.mode !== "gate"}
      <IconButton label={t("review.dismiss")} onclick={() => review.dismiss()} icon="x" />
    {/if}
  </div>
{:else if flash}
  <div class="banner banner--quiet review-bar" role="status">{t("review.allDone")}</div>
{/if}
