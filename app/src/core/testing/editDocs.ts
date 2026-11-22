// Made-up Markdown documents of a size no fixture has, read as Edit mode reads them (blank-line separated blocks),
// for the tests and benches of the Edit-mode editor's marks and edits.
import type { Segment } from "../../api/generated/Segment";
import type { TextRange } from "../editing/session.svelte";

/** Blank-line separated runs of `text` as paragraph segments (no backend lock). */
export function paragraphSegments(text: string): Segment[] {
  const lines = text.split("\n");
  const out: Segment[] = [];
  let start = -1;
  lines.forEach((line, i) => {
    const filled = line.trim() !== "";
    if (filled && start < 0) start = i;
    if (!filled && start >= 0) {
      out.push({ lineStart: start + 1, lineEnd: i, kind: "paragraph", lock: null });
      start = -1;
    }
  });
  if (start >= 0) {
    out.push({ lineStart: start + 1, lineEnd: lines.length, kind: "paragraph", lock: null });
  }
  return out;
}

const WORDS = ["teh", "quick", "brwon", "fox", "jumps", "ovr", "the", "lazy", "dgo", "again"];

/** `count` paragraphs of plain words (a few written with markup), blank lines between them. */
export function paragraphsText(count: number): string {
  const blocks: string[] = [];
  for (let i = 0; i < count; i++) {
    const words = Array.from({ length: 12 }, (_, w) => WORDS[(i + w * 3) % WORDS.length]);
    if (i % 7 === 3) words[2] = `**${words[2]}**`;
    if (i % 11 === 5) words[5] = `[${words[5]}](https://example.com/${i})`;
    blocks.push(`Paragraph ${i} ${words.join(" ")}.`);
  }
  return `${blocks.join("\n\n")}\n`;
}

/** One table of `rows` body rows of plain words, as one block. */
export function tableText(rows: number): string {
  const lines = ["| name | note | more |", "| --- | --- | --- |"];
  for (let i = 0; i < rows; i++) {
    lines.push(`| row ${i} teh | ${WORDS[i % WORDS.length]} ovr dgo | brwon fox ${i} |`);
  }
  return `${lines.join("\n")}\n`;
}

/** Ranges of the misspelled words of `text` (`teh`, `brwon`, `ovr`, `dgo`), every `step`-th one, in text order. */
export function misspelledRanges(text: string, step = 1): TextRange[] {
  const out: TextRange[] = [];
  let seen = 0;
  for (const match of text.matchAll(/\b(?:teh|brwon|ovr|dgo)\b/g)) {
    if (seen++ % step === 0) out.push({ from: match.index, to: match.index + match[0].length });
  }
  return out;
}
