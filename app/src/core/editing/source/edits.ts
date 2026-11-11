// Whole-text edits in the Source editor (correction): the range a scope covers, and checked edits as one transaction.
// Only the replacement text goes in; the selection is mapped through the change.

import { isolateHistory } from "@codemirror/commands";
import { ensureSyntaxTree, syntaxTree } from "@codemirror/language";
import type { EditorState, TransactionSpec } from "@codemirror/state";
import type { EditScope, TextEdit, TextRange } from "../session.svelte";
import { checkEdits } from "../textEdits";

/** Markdown blocks that are prose (a list item's | quote's text is a `Paragraph` inside it; a task item's a `Task`;
 *  a table's is each `TableCell`: its text without the padding and the pipes around it). */
const PROSE = /^(Paragraph|Task|TableCell|ATXHeading[1-6]|SetextHeading[12])$/;
/** How long (ms) the parser may run to reach the caret in a long document. */
const PARSE_MS = 200;

type Node = ReturnType<ReturnType<typeof syntaxTree>["resolveInner"]>;

export function scopeIn(state: EditorState, kind: EditScope): TextRange | null {
  const { from, to, head } = state.selection.main;
  switch (kind) {
    case "selection":
      return from === to ? null : { from, to };
    case "document":
      return { from: 0, to: state.doc.length };
    case "paragraph":
      return blockAt(state, head);
    default:
      return kind satisfies never;
  }
}

/** The prose block holding `pos` (also right at its end), from the editor's Markdown tree. */
function blockAt(state: EditorState, pos: number): TextRange | null {
  const tree =
    ensureSyntaxTree(state, Math.min(state.doc.length, pos + 1), PARSE_MS) ?? syntaxTree(state);
  for (const side of [1, -1] as const) {
    for (let node: Node | null = tree.resolveInner(pos, side); node; node = node.parent) {
      if (PROSE.test(node.name)) return { from: node.from, to: node.to };
    }
  }
  return null;
}

/** One transaction (its own undo step) putting in every edit; `null` when the edits do not pass `checkEdits`. */
export function editsSpec(state: EditorState, edits: readonly TextEdit[]): TransactionSpec | null {
  const sorted = checkEdits(edits, state.doc.length, (from, to) => state.sliceDoc(from, to));
  if (sorted === null) return null;
  return {
    changes: sorted.map(({ from, to, insert }) => ({ from, to, insert })),
    annotations: isolateHistory.of("full"),
    userEvent: "input.correct",
  };
}
