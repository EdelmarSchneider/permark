<script lang="ts">
  // The body of a code | text document while it is in review: the text read-only, the changes in it (see lines.ts).
  // The review store owns what the changes are and what Keep | Revert do; this draws them. The code view loads on
  // first use. It is drawn for the version of the file the review was drawn from, and only for that one.
  import { untrack } from "svelte";
  import { api } from "../../api";
  import { t } from "../../core/i18n";
  import { look } from "../../core/look.svelte";
  import type { ViewerProps } from "../../core/plugins/props";
  import type { LinesView } from "./lines";
  import { review } from "./store.svelte";

  let { view }: ViewerProps = $props();

  // a sheet (a config file) has no text in its body: the text comes from the backend
  let fetched = $state.raw<{ hash: string | null; text: string } | null>(null);
  const own = $derived(view.body.kind === "text" ? view.body.text : null);
  const text = $derived(
    own ?? (fetched !== null && fetched.hash === view.hash ? fetched.text : null),
  );
  const now = $derived(review.state);
  const drawn = $derived(
    now.status === "reviewing" &&
      now.view.unit === "lines" &&
      now.view.path === view.path &&
      now.view.hash === view.hash &&
      text !== null
      ? { review: now.view, at: now.at, text }
      : null,
  );

  let host: HTMLElement | undefined = $state();
  let lines: LinesView | null = null;
  let current = -2;

  // the text of a sheet
  $effect(() => {
    const path = view.path;
    const hash = view.hash;
    if (own !== null) return;
    let stale = false;
    void api.documentText(path).then((result) => {
      if (!stale && result.ok) fetched = { hash, text: result.value };
    });
    return () => {
      stale = true;
    };
  });

  // the code view: made when the element exists, torn down with it
  $effect(() => {
    const parent = host;
    if (!parent) return;
    let gone = false;
    void import("./lines").then(({ createLinesView }) => {
      if (gone) return;
      lines = createLinesView({
        parent,
        path: view.path,
        wrap: look.wrap,
        host: {
          keep: (index) => void review.keep(index),
          revert: (index) => void review.revert(index),
        },
      });
      draw();
    });
    return () => {
      gone = true;
      lines?.destroy();
      lines = null;
      current = -2;
    };
  });

  /** Gives the code view what the review says now. */
  function draw(): void {
    const shown = drawn;
    if (lines === null || shown === null) return;
    lines.show(shown.text, {
      changes: shown.review.changes,
      total: shown.review.count,
      current: shown.at,
      canKeep: shown.review.canKeep,
    });
    if (shown.at !== current) {
      current = shown.at;
      lines.reveal(shown.review.changes[shown.at]);
    }
  }

  // the wrap setting changed: the code view follows
  $effect(() => {
    const on = look.wrap;
    untrack(() => lines?.setWrap(on));
  });

  // what is drawn follows the review (an effect only because the code view is not Svelte)
  $effect(() => {
    void drawn;
    draw();
  });
</script>

<div class="rv-code" role="region" aria-label={t("review.codeLabel")} bind:this={host}></div>
