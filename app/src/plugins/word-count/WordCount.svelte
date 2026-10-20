<script lang="ts">
  // Status bar item: words (or characters, or minutes to read) of the document, or of the selection when there is
  // one. Live while editing (counted here); in View, the file's text is counted once per version. Click: next measure.
  import { api } from "../../api";
  import { createAnalysisText } from "../../core/doc/analysisText.svelte";
  import { language, t, tn } from "../../core/i18n";
  import { measure, minutes, type Counts } from "./count";

  type Unit = "words" | "characters" | "minutes";
  const UNITS: Unit[] = ["words", "characters", "minutes"];
  /** Longer texts are counted by the backend. */
  const LOCAL_LIMIT = 200_000;

  const source = createAnalysisText();
  let unit = $state<Unit>("words");
  let big = $state.raw<Counts | null>(null);

  const text = $derived(source.text);
  const selection = $derived(source.selection);
  const markdown = $derived(source.markdown);
  const countable = $derived(source.countable);
  /** Selection text is plain (Edit, View) except in Source. */
  const selectionKind = $derived(source.selectionMarkdown ? "markdown" : "plain");

  // a long text: the backend counts it (debounced while typing)
  $effect(() => {
    const current = text;
    if (current.length <= LOCAL_LIMIT) return;
    const timer = setTimeout(() => {
      void api.analyze(null, current, markdown ? "markdown" : "plain").then((result) => {
        if (!result.ok) return;
        const words = result.value.find((m) => m.id === "word-count.words")?.value ?? 0;
        const characters = result.value.find((m) => m.id === "word-count.characters")?.value ?? 0;
        big = { words, cjk: 0, characters };
      });
    }, 600);
    return () => clearTimeout(timer);
  });

  const whole = $derived.by((): Counts => {
    if (text.length > LOCAL_LIMIT) return big ?? { words: 0, cjk: 0, characters: 0 };
    return measure(text, markdown ? "markdown" : "plain");
  });
  const part = $derived(selection === "" ? null : measure(selection, selectionKind));

  const number = (n: number): string => new Intl.NumberFormat(language.locale).format(n);

  function valueOf(counts: Counts, of: Unit): number {
    switch (of) {
      case "words":
        return counts.words;
      case "characters":
        return counts.characters;
      case "minutes":
        return minutes(counts);
      default:
        return of satisfies never;
    }
  }

  function label(of: Unit): string {
    const total = valueOf(whole, of);
    if (part !== null && of !== "minutes") {
      return t(of === "words" ? "wordCount.wordsOf" : "wordCount.charactersOf", {
        part: number(valueOf(part, of)),
        total: number(total),
      });
    }
    const n = number(total);
    switch (of) {
      case "words":
        return tn("wordCount.words", total, { n });
      case "characters":
        return tn("wordCount.characters", total, { n });
      case "minutes":
        return tn("wordCount.minutes", total, { n });
      default:
        return of satisfies never;
    }
  }

  const tooltip = $derived(UNITS.map(label).join(" · "));

  function next(): void {
    unit = UNITS[(UNITS.indexOf(unit) + 1) % UNITS.length] ?? "words";
  }
</script>

{#if countable}
  <button type="button" class="count" title={tooltip} aria-label={tooltip} onclick={next}>
    {label(unit)}
  </button>
{/if}

<style>
  .count {
    margin-right: var(--sp-3);
    padding: 0 6px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
    font-variant-numeric: tabular-nums;
  }
  .count:hover {
    background: var(--hover);
  }
</style>
