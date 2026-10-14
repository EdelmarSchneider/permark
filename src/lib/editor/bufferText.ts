// The text of a file as an editor buffer, and back. The buffer has no BOM and `\n` line endings (that is what
// CodeMirror works with); the file's BOM and line ending are kept next to it and put back when saving.

import { BOM, dominantEol, type LineEnding } from "./blockModel";

/** What the buffer lost on the way in and must get back on the way out. */
export type TextFormat = { bom: boolean; eol: LineEnding };

/** The version the buffer started from: the file's text, and the buffer made from it. */
export type BufferBase = { text: string; buffer: string };

export function toBuffer(text: string): { buffer: string; format: TextFormat } {
  const bom = text.startsWith(BOM);
  const rest = bom ? text.slice(BOM.length) : text;
  return {
    buffer: rest.replace(/\r\n|\r/g, "\n"),
    format: { bom, eol: dominantEol(rest) },
  };
}

/** The buffer text with every line terminator as `\n` (what `toBuffer` makes of a file's text). */
function normalized(text: string): string {
  return text.replace(/\r\n|\r/g, "\n");
}

/** The offset in `body` of the character that is `bufferOffset` characters into its buffer (a `\r\n` is one). */
function bodyOffset(body: string, bufferOffset: number): number {
  let at = 0;
  for (let n = 0; n < bufferOffset && at < body.length; n++) {
    at += body.charCodeAt(at) === 13 && body.charCodeAt(at + 1) === 10 ? 2 : 1;
  }
  return at;
}

/** The file text with its terminators set to `eol`, for a buffer that has none but `\n`. */
function withEol(buffer: string, eol: LineEnding): string {
  return eol === "\n" ? buffer : buffer.replace(/\n/g, eol);
}

/** How many times the boundaries of the changed span are moved outwards to keep a `\r` and a `\n` apart. */
const MAX_WIDENINGS = 4;

/**
 * The file text (without its BOM) for a buffer that was made from `base`: what comes before the first change and
 * after the last one is the original text again, byte for byte, and only the changed span is written with the
 * file's line ending.
 */
function spliceBuffer(buffer: string, format: TextFormat, base: BufferBase): string {
  const body = base.text.startsWith(BOM) ? base.text.slice(BOM.length) : base.text;
  if (buffer === base.buffer) return body;
  const old = base.buffer;
  const limit = Math.min(old.length, buffer.length);
  let head = 0;
  while (head < limit && old.charCodeAt(head) === buffer.charCodeAt(head)) head++;
  let tail = 0;
  while (
    tail < limit - head &&
    old.charCodeAt(old.length - 1 - tail) === buffer.charCodeAt(buffer.length - 1 - tail)
  ) {
    tail++;
  }
  for (let widened = 0; widened <= MAX_WIDENINGS; widened++) {
    const start = bodyOffset(body, head);
    const end = bodyOffset(body, old.length - tail);
    const spliced =
      body.slice(0, start) +
      withEol(buffer.slice(head, buffer.length - tail), format.eol) +
      body.slice(end);
    // A `\r` kept before a `\n` that was written (or the reverse) would fuse into one CRLF and swallow a line.
    if (normalized(spliced) === buffer) return spliced;
    if (head > 0) head--;
    if (tail > 0) tail--;
  }
  return withEol(buffer, format.eol);
}

/**
 * The file text for a buffer. With the `base` the buffer started from, the lines that were not touched keep their
 * own line endings (a file that mixes `\n`, `\r\n` and `\r` stays as it was, and a save without an edit writes the
 * very same bytes); what was changed is written with the file's most common one. Without a base every line gets it.
 */
export function fromBuffer(buffer: string, format: TextFormat, base?: BufferBase): string {
  const body = base ? spliceBuffer(buffer, format, base) : withEol(buffer, format.eol);
  return format.bom ? BOM + body : body;
}
