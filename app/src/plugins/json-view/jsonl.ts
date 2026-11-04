// Records of a JSON Lines file from the lines `largeText` already holds. Same blank rule as
// `permark_core::format::records`: a break is `\n`, one trailing `\r` is not part of the value, a line that is empty
// after a trim of spaces and tabs has no index, and the index is 1-based over the rest. This does not parse JSON.

import type { Load } from "../../core/doc/largeText";

export type JsonlRecord = {
  /** 1-based over non-blank lines. */
  index: number;
  /** 1-based physical line. */
  line: number;
  /** The line with one trailing `\r` removed. Not parsed. */
  text: string;
};

/** One trailing CR is not part of the value. */
export function lineValue(raw: string): string {
  return raw.endsWith("\r") ? raw.slice(0, -1) : raw;
}

/** Blank = empty after a trim of spaces and tabs (nothing else). */
export function isBlank(value: string): boolean {
  let start = 0;
  let end = value.length;
  while (start < end && (value.charCodeAt(start) === 32 || value.charCodeAt(start) === 9)) {
    start += 1;
  }
  while (end > start && (value.charCodeAt(end - 1) === 32 || value.charCodeAt(end - 1) === 9)) {
    end -= 1;
  }
  return start === end;
}

/**
 * Records in `lines` (`largeText` lines: split on `\n`, the empty piece after a final break already dropped).
 * `lastFinal` is false while the last line is an unfinished `Held.open` line (the load is not `done` or `capped`):
 * that line is not a record yet.
 */
export function recordsOf(lines: readonly string[], lastFinal: boolean): JsonlRecord[] {
  const end = lines.length - (lastFinal ? 0 : 1);
  const out: JsonlRecord[] = [];
  let index = 0;
  for (let i = 0; i < end; i += 1) {
    const text = lineValue(lines[i] ?? "");
    if (isBlank(text)) continue;
    index += 1;
    out.push({ index, line: i + 1, text });
  }
  return out;
}

/** The last held line counts: it ended on a break, or the load is finished (`done`) or stopped (`capped`). */
export function lastIsFinal(open: boolean, status: Load["status"]): boolean {
  return !open || status === "done" || status === "capped";
}

/** Held text from `linesOf` / `append` (a final break is restored when the last line is closed). */
export function heldText(lines: readonly string[], open: boolean): string {
  if (lines.length === 0) return "";
  const joined = lines.join("\n");
  return open ? joined : `${joined}\n`;
}
