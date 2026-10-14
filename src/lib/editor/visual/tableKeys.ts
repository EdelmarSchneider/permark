// The keys of a table in Visual mode, spreadsheet-like (a Markdown table cell cannot hold a line break, so Enter has no text to split):
//   Enter        the same column, one row down; in the last row, a new row is added and the caret goes into it
//   Shift+Enter  the same column, one row up (nothing in the header row)
//   Mod+Enter    leave the table: into the paragraph right after it, or a new one (Mod is Ctrl, and Cmd on macOS)
//   Tab          the next cell; in the last cell, a new row
// They are tried before the Markdown presets' own keys (whose Enter leaves the table), and only the caret in a table makes them
// take a key, so Enter anywhere else is unchanged.

import type { Ctx } from "@milkdown/kit/ctx";
import { addRowWithAlignment } from "@milkdown/kit/preset/gfm";
import { closeHistory } from "@milkdown/kit/prose/history";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import {
  Selection,
  TextSelection,
  type Command,
  type EditorState,
  type Transaction,
} from "@milkdown/kit/prose/state";
import { isInTable, selectionCell, TableMap, type TableRect } from "@milkdown/kit/prose/tables";

/** The caret at the end of the text of the cell that starts at `cellPos`. */
function endOfCell(doc: PMNode, cellPos: number): Selection {
  const cell = doc.nodeAt(cellPos);
  return Selection.near(doc.resolve(cellPos + (cell?.nodeSize ?? 1) - 1), -1);
}

/** The cell the caret is in, with its table. Null when the caret is not in a table cell. */
function currentCell(state: EditorState): TableRect | null {
  if (!isInTable(state)) return null;
  try {
    const $cell = selectionCell(state);
    const table = $cell.node(-1);
    const tableStart = $cell.start(-1);
    const map = TableMap.get(table);
    return { ...map.findCell($cell.pos - tableStart), tableStart, map, table };
  } catch {
    // a selection that is in a row but in no cell: not one of ours
    return null;
  }
}

/** Adds a row at the end of the table and returns the transaction that also puts the caret in its cell `column`. */
export function appendRow(
  ctx: Ctx,
  state: EditorState,
  at: TableRect,
  column: number,
): Transaction {
  const { tableStart, table, map } = at;
  const rowAt = tableStart + table.content.size;
  // its own undo step: Ctrl+Z takes the row away and leaves what was typed before it
  const tr = closeHistory(addRowWithAlignment(ctx, state.tr, at, map.height));
  const row = tr.doc.nodeAt(rowAt);
  let cellAt = rowAt + 1;
  for (let c = 0; c < column && row; c++) cellAt += row.child(c).nodeSize;
  return tr.setSelection(endOfCell(tr.doc, cellAt)).scrollIntoView();
}

/** Moves the caret to the end of the cell at (`row`, `column`) of the table. */
function moveTo(
  state: EditorState,
  dispatch: ((tr: Transaction) => void) | undefined,
  at: TableRect,
  row: number,
  column: number,
) {
  if (!dispatch) return;
  const cellPos = at.tableStart + at.map.map[row * at.map.width + column];
  dispatch(state.tr.setSelection(endOfCell(state.doc, cellPos)).scrollIntoView());
}

/** Enter: one row down in the same column; in the last row a row is added first. */
export const enterDown =
  (ctx: Ctx): Command =>
  (state, dispatch) => {
    const at = currentCell(state);
    if (!at) return false;
    if (at.bottom < at.map.height) {
      moveTo(state, dispatch, at, at.bottom, at.left);
    } else if (dispatch) {
      dispatch(appendRow(ctx, state, at, at.left));
    }
    return true;
  };

/** Shift+Enter: one row up in the same column. Taken in the header row too, where it does nothing (no line break goes into a cell). */
export const enterUp: Command = (state, dispatch) => {
  const at = currentCell(state);
  if (!at) return false;
  if (at.top > 0) moveTo(state, dispatch, at, at.top - 1, at.left);
  return true;
};

/** Mod+Enter: out of the table, into the paragraph right after it (one is added unless there already is one). */
export const exitTable: Command = (state, dispatch) => {
  const at = currentCell(state);
  if (!at) return false;
  const after = at.tableStart - 1 + at.table.nodeSize;
  const $after = state.doc.resolve(after);
  if ($after.nodeAfter?.type.name === "paragraph") {
    if (dispatch) {
      dispatch(state.tr.setSelection(TextSelection.create(state.doc, after + 1)).scrollIntoView());
    }
    return true;
  }
  const paragraph = state.schema.nodes.paragraph?.createAndFill();
  if (!paragraph || !$after.parent.canReplaceWith($after.index(), $after.index(), paragraph.type)) {
    return true;
  }
  if (dispatch) {
    const tr = state.tr.insert(after, paragraph);
    dispatch(tr.setSelection(TextSelection.create(tr.doc, after + 1)).scrollIntoView());
  }
  return true;
};

/** Tab in the last cell of the table: a new row, and the caret in its first cell. Anywhere else the preset's Tab moves between cells. */
export const tabAtEnd =
  (ctx: Ctx): Command =>
  (state, dispatch) => {
    const at = currentCell(state);
    if (!at || at.bottom !== at.map.height || at.right !== at.map.width) return false;
    if (dispatch) dispatch(appendRow(ctx, state, at, 0));
    return true;
  };

/** The key bindings of a table, for `keydownHandler`. */
export function tableKeymap(ctx: Ctx): Record<string, Command> {
  return {
    Enter: enterDown(ctx),
    "Shift-Enter": enterUp,
    "Mod-Enter": exitTable,
    Tab: tabAtEnd(ctx),
  };
}

/** Whether a key press belongs to an input method that is composing text (Enter then picks a candidate and is not ours). */
export function composing(event: KeyboardEvent, view: { composing: boolean }): boolean {
  return event.isComposing || event.keyCode === 229 || view.composing;
}
