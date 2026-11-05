// Checked whole-text edits (correction), shared by both editors: the edits in text order, only while each range is
// still in the text and still reads its `expect`.

import type { TextEdit } from "./session.svelte";

/** `edits` by position; `null` when none, or any range is out of `0..length`, backwards, overlapping another or no
 *  longer reads its `expect` (`read(from, to)`: the text now). All or nothing. */
export function checkEdits(
  edits: readonly TextEdit[],
  length: number,
  read: (from: number, to: number) => string,
): TextEdit[] | null {
  if (edits.length === 0) return null;
  const sorted = [...edits].sort((a, b) => a.from - b.from);
  let end = 0;
  for (const edit of sorted) {
    if (edit.from < end || edit.to < edit.from || edit.to > length) return null;
    if (read(edit.from, edit.to) !== edit.expect) return null;
    end = edit.to;
  }
  return sorted;
}
