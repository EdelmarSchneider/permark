// Small text helpers shared by quick open and find.

/**
 * Lowercases and strips accents one UTF-16 unit at a time, so the result has the same length as the
 * input and indices stay valid for highlighting.
 */
export function foldChars(text: string): string {
  let out = "";
  for (let i = 0; i < text.length; i++) {
    const unit = text[i];
    const base = unit.normalize("NFD")[0] ?? unit;
    out += base.toLowerCase()[0] ?? base;
  }
  return out;
}

export type Segment = { text: string; match: boolean };

/** Splits `text` into runs so matched characters can be rendered in bold. `positions` index into `text`. */
export function highlightSegments(text: string, positions: Iterable<number>): Segment[] {
  const hit = new Set(positions);
  const segments: Segment[] = [];
  for (let i = 0; i < text.length; i++) {
    const match = hit.has(i);
    const last = segments[segments.length - 1];
    if (last && last.match === match) last.text += text[i];
    else segments.push({ text: text[i], match });
  }
  return segments;
}

/** Small stable string hash (djb2), used to compare blocks when the backend sent no per-block hashes. */
export function hashString(text: string): string {
  let hash = 5381;
  for (let i = 0; i < text.length; i++) hash = ((hash << 5) + hash + text.charCodeAt(i)) >>> 0;
  return hash.toString(16).padStart(8, "0");
}
