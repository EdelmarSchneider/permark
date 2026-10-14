// The block model of the editor (design: docs/design/v0.2.md §3). A document is its text split
// into the top-level blocks that Rust found (`parse_blocks`) and the separators between them, so
// that `blocks + separators` give back the text byte for byte (CRLF, a BOM and the final newline
// included). The editor then rewrites only the blocks a person changed: `compose()` takes the
// original bytes for every block that was not touched, and writes new text for the others.
//
// Rust measures blocks in UTF-8 bytes; strings here are UTF-16, so the text is cut on its bytes.

import { diffBlocks } from "../diff";
import type { BlockKind, ParsedBlocks } from "../types";

export const BOM = "﻿";
export type LineEnding = "\n" | "\r\n" | "\r";

/** One top-level block: its exact text (no line terminator at the ends) and the lines it covers (0-based). */
export type MapBlock = { kind: BlockKind; text: string; startLine: number; endLine: number };

/**
 * `seps[i]` is the text before block `i` (so `seps[0]` holds a BOM) and `seps[blocks.length]` the text after
 * the last block (the final newline). Without blocks, `seps` is the whole text.
 */
export type BlockMap = { blocks: MapBlock[]; seps: string[]; eol: LineEnding };

const encoder = new TextEncoder();
// `ignoreBOM` keeps a BOM in the decoded text instead of dropping it.
const decoder = new TextDecoder("utf-8", { ignoreBOM: true });

/** The number of line terminators (`\n`, `\r\n` or a lone `\r`) in a text. */
export function countLineBreaks(text: string): number {
  let breaks = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    if (c === 10) breaks++;
    else if (c === 13) {
      if (text.charCodeAt(i + 1) === 10) i++;
      breaks++;
    }
  }
  return breaks;
}

/** The line ending used most: CRLF or a lone CR when it beats the others, else LF (also on a tie). */
export function dominantEol(text: string): LineEnding {
  let crlf = 0;
  let lf = 0;
  let cr = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    if (c === 10) lf++;
    else if (c === 13) {
      if (text.charCodeAt(i + 1) === 10) {
        crlf++;
        i++;
      } else cr++;
    }
  }
  if (cr > lf && cr > crlf) return "\r";
  return crlf > lf ? "\r\n" : "\n";
}

/** Whether the ranges are in order, inside the text, and not empty. */
function validRanges(blocks: ParsedBlocks["blocks"], length: number): boolean {
  let at = 0;
  for (const b of blocks) {
    if (!(b.startByte >= at && b.endByte > b.startByte && b.endByte <= length)) return false;
    at = b.endByte;
  }
  return true;
}

/**
 * Cuts `text` into blocks and separators along the byte ranges from Rust. Ranges that do not fit the text
 * (they cannot come from `parse_blocks`) give a map without blocks: the whole text is one separator, so
 * nothing can be rewritten by mistake.
 */
export function buildBlockMap(text: string, parsed: ParsedBlocks): BlockMap {
  const eol = dominantEol(text);
  const bytes = encoder.encode(text);
  const whole: BlockMap = { blocks: [], seps: [text], eol };
  if (!validRanges(parsed.blocks, bytes.length)) return whole;

  const blocks: MapBlock[] = [];
  const seps: string[] = [];
  let at = 0;
  let line = 0;
  let total = 0;
  for (const range of parsed.blocks) {
    const sep = decoder.decode(bytes.subarray(at, range.startByte));
    const blockText = decoder.decode(bytes.subarray(range.startByte, range.endByte));
    line += countLineBreaks(sep);
    const startLine = line;
    line += countLineBreaks(blockText);
    seps.push(sep);
    blocks.push({ kind: range.kind, text: blockText, startLine, endLine: line });
    total += sep.length + blockText.length;
    at = range.endByte;
  }
  const last = decoder.decode(bytes.subarray(at));
  seps.push(last);
  total += last.length;
  // A range that cut a character in two would have been decoded with replacement characters.
  if (total !== text.length) return whole;
  return { blocks, seps, eol };
}

/** The text the map was built from. */
export function joinBlockMap(map: BlockMap): string {
  let out = map.seps[0] ?? "";
  for (let i = 0; i < map.blocks.length; i++) out += map.blocks[i].text + map.seps[i + 1];
  return out;
}

/**
 * One block of the document being saved: `origin` is the block of the original it came from (null for a
 * new block), `text` is its new text, or undefined while it is untouched (its original bytes are kept).
 */
export type Slot = { origin: number | null; text?: string };

/** Text from the editor as a block: the file's line endings and no line break at its ends. */
export function tidyBlockText(text: string, eol: LineEnding): string {
  const unified = text.replace(/\r\n|\r|\n/g, eol);
  let end = unified.length;
  while (end > 0 && (unified[end - 1] === "\n" || unified[end - 1] === "\r")) end--;
  return unified.slice(0, end);
}

/** The lines of a separator that are not blank: what a separator holds besides spacing (link reference definitions). */
function contentLines(separator: string): string[] {
  return separator.split(/\r\n|\r|\n/).filter((line) => line.trim() !== "");
}

/**
 * The text to save. Untouched blocks come back as their original bytes. A separator is kept when both
 * neighbours were next to each other in the original (whether or not either was edited), so blank lines
 * the author chose survive; otherwise the blocks are one blank line apart. A BOM and the final newline are
 * kept as they were.
 *
 * Text that is not a block but lives between blocks (link reference definitions) must outlive the blocks
 * around it: when a separator is dropped because its neighbours are no longer next to each other, its
 * non-blank lines are written after the last block that came from before it (before the first block when
 * there is none), one blank line away from the blocks. A document without blocks keeps its text the same way.
 */
export function compose(map: BlockMap, slots: readonly Slot[]): string {
  return composeDetailed(map, slots).text;
}

/** The composed text, and the offset in it where the text of each slot starts (for placing a caret in the new text). */
export type Composed = { text: string; starts: number[] };

/** `compose()` that also says where every slot landed. */
export function composeDetailed(map: BlockMap, slots: readonly Slot[]): Composed {
  const count = map.blocks.length;
  // a document without blocks is all separator: nothing to compose unless blocks are added
  if (count === 0 && slots.length === 0) return { text: map.seps[0] ?? "", starts: [] };
  const { eol } = map;
  const gap = eol + eol;
  const isAdjacent = (i: number): boolean => {
    const [previous, slot] = [slots[i - 1], slots[i]];
    return previous.origin !== null && slot.origin !== null && slot.origin === previous.origin + 1;
  };

  // `rescued[i]` is written before slot `i` (the last entry: after the last slot)
  const rescued: string[][] = Array.from({ length: slots.length + 1 }, () => []);
  let out: string;
  let trailing: string;
  if (count > 0) {
    out = map.seps[0];
    trailing = map.seps[count];
    const kept = new Set<number>();
    for (let i = 1; i < slots.length; i++) if (isAdjacent(i)) kept.add(slots[i].origin as number);
    for (let j = 1; j < count; j++) {
      if (kept.has(j)) continue;
      const lines = contentLines(map.seps[j]);
      if (lines.length === 0) continue;
      let after = -1;
      slots.forEach((slot, s) => {
        if (slot.origin !== null && slot.origin < j) after = s;
      });
      rescued[after + 1].push(...lines);
    }
  } else {
    const bom = map.seps[0]?.startsWith(BOM) ?? false;
    out = bom ? BOM : "";
    trailing = eol;
    rescued[0] = contentLines((map.seps[0] ?? "").slice(bom ? BOM.length : 0));
  }

  const starts: number[] = [];
  slots.forEach((slot, i) => {
    if (i > 0) {
      out += isAdjacent(i)
        ? map.seps[slot.origin as number]
        : gap + (rescued[i].length > 0 ? rescued[i].join(eol) + gap : "");
    } else if (rescued[0].length > 0) {
      out += rescued[0].join(eol) + gap;
    }
    starts.push(out.length);
    if (slot.text !== undefined) out += tidyBlockText(slot.text, eol);
    else if (slot.origin !== null && map.blocks[slot.origin]) out += map.blocks[slot.origin].text;
    else throw new Error("a slot without text needs an original block");
  });
  const tail = rescued[slots.length];
  if (slots.length === 0) out += tail.join(eol);
  else if (tail.length > 0) out += gap + tail.join(eol);
  return { text: out + trailing, starts };
}

/** A slot for every block of the map, all untouched: composing them gives back the original. */
export function untouchedSlots(map: BlockMap): Slot[] {
  return map.blocks.map((_, origin) => ({ origin }));
}

/** How the blocks of a new text relate to those of the original. */
export type BlockMatch = {
  /** For every block of the new text, the original block with the same text, or null when it is new or changed. */
  origins: (number | null)[];
  /** Blocks of the new text that are new or changed. */
  added: number;
  /** Blocks of the original that are gone or changed. */
  removed: number;
  /** For every original block, the block of the new text it became (or null). */
  moved: (number | null)[];
};

/** Exact text equality of blocks, found with the same LCS as the live diff. */
export function matchBlocks(base: BlockMap, next: BlockMap): BlockMatch {
  const ids = new Map<string, string>();
  const key = (b: MapBlock): string => {
    let id = ids.get(b.text);
    if (id === undefined) {
      id = String(ids.size);
      ids.set(b.text, id);
    }
    return id;
  };
  const diff = diffBlocks(base.blocks.map(key), next.blocks.map(key));
  const origins: (number | null)[] = next.blocks.map(() => null);
  const moved: (number | null)[] = base.blocks.map(() => null);
  for (const [from, to] of diff.matches) {
    origins[to] = from;
    moved[from] = to;
  }
  const removed = diff.removed.reduce((sum, run) => sum + run.oldIndices.length, 0);
  return { origins, added: diff.added.length, removed, moved };
}

/** "Saved, 2 blocks changed": how many blocks the save touched (0 when only blank lines changed). */
export function changedBlockCount(match: BlockMatch): number {
  return Math.max(match.added, match.removed);
}

/** The index of the block that holds a line, or the last block before it (-1 before the first). */
export function blockAtLine(map: BlockMap, line: number): number {
  let found = -1;
  for (let i = 0; i < map.blocks.length; i++) {
    if (map.blocks[i].startLine > line) break;
    found = i;
  }
  return found;
}

/**
 * Where a line of the original text is in the new text: the same line of the same block when the block
 * survived (so edits above it do not move the caret away), else the nearest surviving block above it.
 */
export function mapLine(base: BlockMap, next: BlockMap, match: BlockMatch, line: number): number {
  const index = blockAtLine(base, line);
  if (index < 0 || next.blocks.length === 0) return Math.max(0, line);
  let origin = index;
  let target = match.moved[index];
  while (target === null && origin > 0) {
    origin--;
    const earlier = match.moved[origin];
    if (earlier !== null) target = Math.min(earlier + (index - origin), next.blocks.length - 1);
  }
  if (target === null) target = Math.min(index, next.blocks.length - 1);
  const into = next.blocks[target];
  const offset = Math.max(0, line - base.blocks[index].startLine);
  return Math.min(into.startLine + offset, into.endLine);
}
