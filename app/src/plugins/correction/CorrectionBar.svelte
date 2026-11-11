<script lang="ts">
  // The correction bar: the piece review bar of `local-ai` (`local-ai/pieces/ReviewBar.svelte`) with correction's
  // words and its excerpt: the current suggestion in its text (removed | added words marked), the store's cut (its
  // block, word edges, `…`).
  import { t } from "../../core/i18n";
  import ReviewBar, { type ReviewLabels } from "../local-ai/pieces/ReviewBar.svelte";
  import type { Found } from "../local-ai/pieces/review.svelte";
  import { correction } from "./store.svelte";

  /** What a screen reader hears for the current suggestion. */
  function spoken(s: Found, at: number, n: number): string {
    const params = { i: at + 1, n, a: s.original, b: s.insert };
    if (s.original === "") return t("correction.sr.insert", params);
    if (s.insert === "") return t("correction.sr.remove", params);
    return t("correction.sr.replace", params);
  }

  const LABELS: ReviewLabels = {
    bar: "correction.bar",
    nothing: "correction.nothing",
    loading: "correction.loading",
    checking: "correction.checking",
    clean: "correction.clean",
    found: "correction.found",
    stopped: "correction.stopped",
    failed: "correction.failed",
    noModel: "correction.failed.noModel",
    position: "correction.position",
    prev: "correction.prev",
    next: "correction.next",
    show: "correction.show",
    discarded: "correction.discarded",
    outdated: "correction.outdated",
    retry: "correction.retry",
    openSettings: "correction.openSettings",
    accept: "correction.accept",
    reject: "correction.reject",
    acceptAll: "correction.acceptAll",
    rejectAll: "correction.rejectAll",
    done: "correction.done",
    spoken,
  };
</script>

<ReviewBar review={correction} labels={LABELS}>
  {#snippet excerpt(s: Found)}{s.before}<del>{s.original}</del><ins>{s.insert}</ins
    >{s.after}{/snippet}
</ReviewBar>

<style>
  del {
    background: var(--review-del-bg);
    color: var(--git-deleted-text);
  }
  ins {
    background: var(--review-add-bg);
    color: var(--git-added-text);
  }
</style>
