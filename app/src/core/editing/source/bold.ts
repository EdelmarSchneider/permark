// Ctrl+B in the Markdown source editor: bold, as in the visual editor. Each selection is wrapped in `**` (an empty one
// gets a pair with the caret inside); a selection with `**` right around it loses them (bold off). Only those
// characters change (surgical); one transaction (one undo step). Pure over CodeMirror state.

import { EditorSelection, type EditorState, type TransactionSpec } from "@codemirror/state";

const MARK = "**";

export function toggleBold(state: EditorState): TransactionSpec {
  const n = MARK.length;
  return state.changeByRange((range) => {
    const before = state.sliceDoc(Math.max(0, range.from - n), range.from);
    const after = state.sliceDoc(range.to, range.to + n);
    if (!range.empty && before === MARK && after === MARK) {
      return {
        changes: [
          { from: range.from - n, to: range.from, insert: "" },
          { from: range.to, to: range.to + n, insert: "" },
        ],
        range: EditorSelection.range(range.from - n, range.to - n),
      };
    }
    return {
      changes: [
        { from: range.from, insert: MARK },
        { from: range.to, insert: MARK },
      ],
      range: EditorSelection.range(range.from + n, range.to + n),
    };
  });
}
