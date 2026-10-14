// The text of a file as an editor buffer, and back. The buffer has no BOM and `\n` line endings (that is what
// CodeMirror works with); the file's BOM and line ending are kept next to it and put back when saving.

import { BOM, dominantEol, type LineEnding } from "./blockModel";

/** What the buffer lost on the way in and must get back on the way out. */
export type TextFormat = { bom: boolean; eol: LineEnding };

export function toBuffer(text: string): { buffer: string; format: TextFormat } {
  const bom = text.startsWith(BOM);
  const rest = bom ? text.slice(BOM.length) : text;
  return {
    buffer: rest.replace(/\r\n|\r/g, "\n"),
    format: { bom, eol: dominantEol(rest) },
  };
}

/**
 * The file text for a buffer. A file with one kind of line ending comes back byte for byte; a file that mixes
 * them gets its most common one on every line.
 */
export function fromBuffer(buffer: string, format: TextFormat): string {
  const body = format.eol === "\r\n" ? buffer.replace(/\n/g, "\r\n") : buffer;
  return format.bom ? BOM + body : body;
}
