// Secret values of a very long line, found window by window: the scan of a whole line grows faster than its length,
// so a line over LONG_LINE is scanned in windows of WINDOW characters (OVERLAP more on both sides: a key just before,
// a value running past), only the windows a drawn piece needs, each once (the caller keeps the cache per line).

import { findCodeSecrets, type Span } from "../../core/config/secrets";

/** A line longer than this is scanned by windows. */
export const LONG_LINE = 4000;
const WINDOW = 2048;
const OVERLAP = 512;

/** The spans found per window of one line (window index -> spans that start in it). */
export type WindowCache = Map<number, Span[]>;

/** Spans (in the line) that start in window `k`. */
function windowSpans(line: string, k: number): Span[] {
  const core = k * WINDOW;
  const start = Math.max(0, core - OVERLAP);
  const end = Math.min(line.length, core + WINDOW + OVERLAP);
  const out: Span[] = [];
  for (const span of findCodeSecrets(line.slice(start, end))) {
    const from = start + span.from;
    if (from < core || from >= core + WINDOW) continue;
    let to = start + span.to;
    // a value cut by the window's end runs on to the next space
    if (to === end) while (to < line.length && !/\s/.test(line.charAt(to))) to += 1;
    out.push({ from, to });
  }
  return out;
}

/** Spans (in the line, in order, not overlapping) that reach into `from` .. `to` (exclusive). */
export function longLineSpans(line: string, from: number, to: number, cache: WindowCache): Span[] {
  const out: Span[] = [];
  // a value begun in the window before may reach into the range
  const first = Math.max(0, Math.floor(from / WINDOW) - 1);
  const last = Math.floor(Math.max(from, to - 1) / WINDOW);
  for (let k = first; k <= last; k += 1) {
    let spans = cache.get(k);
    if (spans === undefined) {
      spans = windowSpans(line, k);
      cache.set(k, spans);
    }
    for (const span of spans) {
      const prev = out[out.length - 1];
      if (span.to > from && span.from < to && (prev === undefined || span.from >= prev.to)) {
        out.push(span);
      }
    }
  }
  return out;
}
