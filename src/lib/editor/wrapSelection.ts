// Ctrl+B, Ctrl+I and Ctrl+K in the Source editor: wrap the selection in Markdown syntax, or take it off again.
// Pure functions over the text, so they are tested without an editor; the CodeMirror glue applies the result.

/** A replacement of `from..to` by `insert`, and where the selection goes afterwards (positions in the new text). */
export type WrapEdit = {
  from: number;
  to: number;
  insert: string;
  selection: { anchor: number; head: number };
};

/**
 * Wraps `from..to` of `doc` in `marker` (`**` for bold, `*` for italic, `~~` for strike), or unwraps it when it
 * already is: either the markers sit just outside the selection, or the selection includes them.
 */
export function toggleMarker(doc: string, from: number, to: number, marker: string): WrapEdit {
  const n = marker.length;
  const selected = doc.slice(from, to);
  const before = doc.slice(Math.max(0, from - n), from);
  const after = doc.slice(to, to + n);
  // `*` inside `**` is not italic: the character beyond the marker must not be the marker's own character
  const lone = (outside: string): boolean => n > 1 || outside !== marker;
  const outer =
    before === marker &&
    after === marker &&
    lone(doc.slice(Math.max(0, from - n - 1), Math.max(0, from - n))) &&
    lone(doc.slice(to + n, to + n + 1));
  if (outer) {
    return {
      from: from - n,
      to: to + n,
      insert: selected,
      selection: { anchor: from - n, head: from - n + selected.length },
    };
  }
  const inner =
    selected.length >= 2 * n &&
    selected.startsWith(marker) &&
    selected.endsWith(marker) &&
    lone(selected.slice(n, n + 1)) &&
    lone(selected.slice(selected.length - n - 1, selected.length - n));
  if (inner) {
    const text = selected.slice(n, selected.length - n);
    return {
      from,
      to,
      insert: text,
      selection: { anchor: from, head: from + text.length },
    };
  }
  const insert = marker + selected + marker;
  return {
    from,
    to,
    insert,
    selection:
      from === to
        ? { anchor: from + n, head: from + n }
        : { anchor: from + n, head: from + n + selected.length },
  };
}

/**
 * Ctrl+K: `[selection](url)` with `url` selected to be typed over; without a selection `[]()` with the caret in the
 * brackets. A selection that already is a web address becomes the target instead: `[](address)`.
 */
export function wrapLink(doc: string, from: number, to: number): WrapEdit {
  const selected = doc.slice(from, to);
  if (selected === "") {
    return { from, to, insert: "[]()", selection: { anchor: from + 1, head: from + 1 } };
  }
  if (/^(https?:\/\/|mailto:)\S+$/i.test(selected)) {
    return {
      from,
      to,
      insert: `[](${selected})`,
      selection: { anchor: from + 1, head: from + 1 },
    };
  }
  const insert = `[${selected}](url)`;
  const start = from + selected.length + 3;
  return { from, to, insert, selection: { anchor: start, head: start + 3 } };
}
