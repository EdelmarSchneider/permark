<script lang="ts">
  // The translation bar: the piece review bar of `local-ai` (`local-ai/pieces/ReviewBar.svelte`) with translation's
  // words and its excerpt: the part as it is over its translation, two whole texts, so each stands on its own (cut to
  // three lines until asked for more) and the excerpt scrolls past a share of the window's height. The languages
  // named are the ones the run under review was sent, not what was chosen since.
  import { language, t, tn, type MessageKey } from "../../core/i18n";
  import ReviewBar, { type ReviewLabels } from "../local-ai/pieces/ReviewBar.svelte";
  import type { Found } from "../local-ai/pieces/review.svelte";
  import ExcerptBlock from "./ExcerptBlock.svelte";
  import { namesIn } from "./languages";
  import { translation } from "./store.svelte";

  const sent = $derived(translation.languages.sent);
  /** Tag of the language translated into (before any run: the one chosen). */
  const target = $derived(sent?.target.tag ?? translation.languages.target);
  /** Tag of the language translated from, when the person named it. */
  const source = $derived(sent?.source?.tag ?? null);
  const asLabel = $derived(namesIn(language.locale));
  const inSentence = $derived(namesIn(language.locale, "sentence"));

  /** A block's name: what it holds, then its language when that is known. */
  function named(what: MessageKey, tag: string | null): string {
    return tag === null
      ? t(what)
      : t("translation.named", { what: t(what), language: asLabel(tag) });
  }

  const LABELS: ReviewLabels = {
    bar: "translation.bar",
    nothing: "translation.nothing",
    loading: "translation.loading",
    checking: "translation.checking",
    clean: "translation.clean",
    found: "translation.found",
    stopped: "translation.stopped",
    failed: "translation.failed",
    noModel: "translation.failed.noModel",
    position: "translation.position",
    prev: "translation.prev",
    next: "translation.next",
    show: "translation.show",
    discarded: "translation.discarded",
    same: (n) => tn("translation.same", n, { language: target === null ? "" : inSentence(target) }),
    outdated: "translation.outdated",
    retry: "translation.retry",
    openSettings: "translation.openSettings",
    accept: "translation.accept",
    reject: "translation.reject",
    acceptAll: "translation.acceptAll",
    rejectAll: "translation.rejectAll",
    done: "translation.done",
    spoken: (s, at, n) => t("translation.spoken", { i: at + 1, n, text: s.insert }),
  };
</script>

<ReviewBar review={translation} labels={LABELS} excerptAs="own">
  {#snippet excerpt(s: Found)}
    <div class="excerpt">
      <!-- a new suggestion starts cut again -->
      {#key s.key}
        <ExcerptBlock
          kind="original"
          label={named("translation.original", source)}
          text={s.original}
          lang={source}
          show={() => translation.show()}
        />
        <ExcerptBlock
          kind="translation"
          label={named("translation.translation", target)}
          text={s.insert}
          lang={target}
          show={() => translation.show()}
        />
      {/key}
    </div>
  {/snippet}
</ReviewBar>

<style>
  .excerpt {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    max-height: 40vh;
    /* room for the texts' own padding, so their words start where the bar's do */
    margin: 0 calc(-1 * var(--sp-1));
    padding: 0 var(--sp-1);
    overflow-y: auto;
  }
</style>
