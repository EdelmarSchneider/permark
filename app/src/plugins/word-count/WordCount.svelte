<script lang="ts">
  // Status bar item: words (or characters, or minutes to read) of the document, or of the selection when there is
  // one. Live while editing (counted here); in View, the file's text is counted once per version. Texts and selections
  // longer than LOCAL_LIMIT are counted by the backend once they rest. Click: next measure.
  import { api, type TextKind } from "../../api";
  import { createAnalysisText } from "../../core/doc/analysisText.svelte";
  import { formatLocale, t, tn } from "../../core/i18n";
  import { LOCAL_LIMIT, measure, minutes, type Counts } from "./count";

  type Unit = "words" | "characters" | "minutes";
  const UNITS: Unit[] = ["words", "characters", "minutes"];

  const source = createAnalysisText();
  let unit = $state<Unit>("words");
  /** The backend's count of a long text, for the document it was asked about. */
  let big = $state.raw<{ path: string | null; counts: Counts } | null>(null);
  /** The backend's count of a long selection, for the very text it was asked about. */
  let bigPart = $state.raw<{ text: string; counts: Counts } | null>(null);
  /** Asks made so far: an answer to an older one never replaces the answer to a newer one. */
  let wholeAsks = 0;
  let partAsks = 0;

  const text = $derived(source.text);
  const selection = $derived(source.selection);
  const markdown = $derived(source.markdown);
  const countable = $derived(source.countable);
  /** Selection text is plain (Edit, View) except in Source. */
  const selectionKind = $derived<TextKind>(source.selectionMarkdown ? "markdown" : "plain");

  /** The backend's words and characters of `subject` (`null`: it could not count). */
  async function counted(subject: string, kind: TextKind): Promise<Counts | null> {
    const result = await api.analyze(null, subject, kind);
    if (!result.ok) return null;
    const words = result.value.find((m) => m.id === "word-count.words")?.value ?? 0;
    const characters = result.value.find((m) => m.id === "word-count.characters")?.value ?? 0;
    return { words, cjk: 0, characters };
  }

  // a long text: the backend counts it (debounced while typing)
  $effect(() => {
    const current = text;
    const path = source.path;
    const kind: TextKind = markdown ? "markdown" : "plain";
    if (current.length <= LOCAL_LIMIT) return;
    const timer = setTimeout(() => {
      const ask = ++wholeAsks;
      void counted(current, kind).then((counts) => {
        if (counts !== null && ask === wholeAsks) big = { path, counts };
      });
    }, 600);
    return () => clearTimeout(timer);
  });

  // a long selection: the backend counts it once the selection rests (measuring it here held the window)
  $effect(() => {
    const current = selection;
    const kind = selectionKind;
    if (current.length <= LOCAL_LIMIT) return;
    const timer = setTimeout(() => {
      const ask = ++partAsks;
      void counted(current, kind).then((counts) => {
        if (counts !== null && ask === partAsks) bigPart = { text: current, counts };
      });
    }, 600);
    return () => clearTimeout(timer);
  });

  const whole = $derived.by((): Counts => {
    if (text.length > LOCAL_LIMIT) {
      // another document's count is no stand-in for this one
      return big !== null && big.path === source.path
        ? big.counts
        : { words: 0, cjk: 0, characters: 0 };
    }
    return measure(text, markdown ? "markdown" : "plain");
  });
  /** `null`: nothing selected, or a long selection the backend has not counted yet. */
  const part = $derived.by((): Counts | null => {
    if (selection === "") return null;
    if (selection.length > LOCAL_LIMIT) return bigPart?.text === selection ? bigPart.counts : null;
    return measure(selection, selectionKind);
  });

  const number = (n: number): string => new Intl.NumberFormat(formatLocale()).format(n);

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
