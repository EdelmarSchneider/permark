// What the token count shows, as pure functions: which of the status item's faces applies, where the front matter ends,
// the line ranges of the outline's sections, the words for a vocabulary that is not there.

import type { VocabEntry, VocabFailure } from "../../api";
import type { MessageKey } from "../../core/i18n";
import type { TokenizerInfo } from "./tokenizers";

/** A number and whether it is only an estimate (an estimator's, or the stand-in while an exact count is worked out). */
export type Figure = { value: number; approx: boolean };

/** Line number (1-based) of the line that closes the front matter block; 0 = the text has none. */
export function frontMatterEnd(text: string): number {
  const lines = text.split("\n", 400).map((l) => (l.endsWith("\r") ? l.slice(0, -1) : l));
  const delimiter = lines[0]?.trimEnd();
  if (delimiter !== "---" && delimiter !== "+++") return 0;
  const close = lines
    .slice(1)
    .findIndex((l) => l.trimEnd() === delimiter || (delimiter === "---" && l.trimEnd() === "..."));
  return close < 0 ? 0 : close + 2;
}

/** Index in `text` where line `line` (1-based) starts; `text.length` when the text has fewer lines. */
export function lineStart(text: string, line: number): number {
  let at = 0;
  for (let n = 1; n < line; n++) {
    const next = text.indexOf("\n", at);
    if (next < 0) return text.length;
    at = next + 1;
  }
  return at;
}

/** Number of lines of a text split at `\n`. */
export function lineCount(text: string): number {
  let n = 1;
  for (let at = text.indexOf("\n"); at >= 0; at = text.indexOf("\n", at + 1)) n += 1;
  return n;
}

/**
 * The lines of each section of the outline, both ends included: from its heading to the line before the next heading
 * of the same or a higher level (subsections are part of it), else to the last line. What an agent's `read` of the
 * section returns.
 */
export function sectionRanges(
  items: readonly { level: number; line: number }[],
  lastLine: number,
): [number, number][] {
  return items.map((item, at) => {
    const next = items.slice(at + 1).find((other) => other.level <= item.level);
    const to = next === undefined ? lastLine : next.line - 1;
    return [item.line, Math.max(item.line, to)];
  });
}

/** What the status item shows. */
export type Display =
  /** No document with text: nothing. */
  | { kind: "none" }
  /** Nothing chosen: the chip that opens the chooser. */
  | { kind: "choose" }
  /** An exact tokenizer whose vocabulary is not on this computer. */
  | { kind: "download"; tokenizer: TokenizerInfo; bytes: number }
  | { kind: "downloading"; tokenizer: TokenizerInfo; percent: number }
  | { kind: "failed"; tokenizer: TokenizerInfo }
  | { kind: "count"; tokenizer: TokenizerInfo; whole: Figure; selection: Figure | null };

export function percentOf(done: number, total: number): number {
  return total <= 0 ? 0 : Math.min(100, Math.floor((done / total) * 100));
}

export function displayOf(input: {
  countable: boolean;
  primary: TokenizerInfo | null;
  /** The vocabulary of the primary tokenizer, when it is an exact one (`null` = not told yet). */
  vocab: VocabEntry | null;
  whole: Figure;
  selection: Figure | null;
}): Display {
  const { primary, vocab } = input;
  if (!input.countable) return { kind: "none" };
  if (primary === null) return { kind: "choose" };
  if (primary.exact && vocab !== null) {
    switch (vocab.state.kind) {
      case "absent":
        return { kind: "download", tokenizer: primary, bytes: vocab.bytes };
      case "downloading":
        return {
          kind: "downloading",
          tokenizer: primary,
          percent: percentOf(vocab.state.done, vocab.state.total),
        };
      case "failed":
        return { kind: "failed", tokenizer: primary };
      case "ready":
        break;
      default:
        return vocab.state satisfies never;
    }
  }
  return { kind: "count", tokenizer: primary, whole: input.whole, selection: input.selection };
}

/** The sentence for why a vocabulary is not there. */
export function failureKey(reason: VocabFailure): MessageKey {
  switch (reason.kind) {
    case "offline":
      return "tokens.vocab.failed.offline";
    case "http":
      return "tokens.vocab.failed.http";
    case "hash":
      return "tokens.vocab.failed.hash";
    case "disk":
      return "tokens.vocab.failed.disk";
    default:
      return reason satisfies never;
  }
}

/** `3.6 MB` for a download size (decimal units, one decimal). */
export function formatMegabytes(bytes: number, locale: string): string {
  const mb = new Intl.NumberFormat(locale, { maximumFractionDigits: 1, minimumFractionDigits: 1 });
  return `${mb.format(bytes / 1_000_000)} MB`;
}
