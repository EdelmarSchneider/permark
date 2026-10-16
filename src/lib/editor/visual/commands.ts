// What the toolbar, the slash menu and the keys do in Visual mode, and how the toolbar reads the selection to show what is
// active. The commands are Milkdown's own (headings, marks, lists, tables, code blocks); this file adds the toggles the
// presets leave out (lifting out of a list, a task list, leaving a quote or a code block).

import { commandsCtx } from "@milkdown/kit/core";
import {
  createCodeBlockCommand,
  insertHrCommand,
  insertImageCommand,
  toggleEmphasisCommand,
  toggleInlineCodeCommand,
  toggleStrongCommand,
  turnIntoTextCommand,
  wrapInBlockquoteCommand,
  wrapInBulletListCommand,
  wrapInHeadingCommand,
  wrapInOrderedListCommand,
} from "@milkdown/kit/preset/commonmark";
import {
  addColAfterCommand,
  addColBeforeCommand,
  addRowAfterCommand,
  addRowBeforeCommand,
  insertTableCommand,
  toggleStrikethroughCommand,
} from "@milkdown/kit/preset/gfm";
import { lift } from "@milkdown/kit/prose/commands";
import type { MarkType, Node as PMNode } from "@milkdown/kit/prose/model";
import { liftListItem } from "@milkdown/kit/prose/schema-list";
import { NodeSelection, type EditorState } from "@milkdown/kit/prose/state";
import { deleteColumn, deleteRow, deleteTable } from "@milkdown/kit/prose/tables";
import type { $Command } from "@milkdown/kit/utils";
import { sanitizeInfo } from "../../code/languages";
import { LOCKED, type Core } from "./core";
import {
  NO_FORMAT,
  tableUnavailable,
  type BlockName,
  type CommandId,
  type FormatState,
  type TableCommandId,
} from "./format";

function markActive(state: EditorState, type: MarkType | undefined): boolean {
  if (!type) return false;
  const { from, to, empty, $from } = state.selection;
  return empty
    ? !!type.isInSet(state.storedMarks ?? $from.marks())
    : state.doc.rangeHasMark(from, to, type);
}

/** Reads the selection for the toolbar. */
export function formatState(state: EditorState): FormatState {
  const { selection } = state;
  const { $from } = selection;
  const marks = state.schema.marks;
  const out: FormatState = {
    ...NO_FORMAT,
    bold: markActive(state, marks.strong),
    italic: markActive(state, marks.emphasis),
    strike: markActive(state, marks.strike_through),
    code: markActive(state, marks.inlineCode ?? marks.code_inline),
    link: markActive(state, marks.link),
    atom: selection instanceof NodeSelection && selection.node.type.name === LOCKED,
  };
  for (let depth = $from.depth; depth > 0; depth--) {
    const node = $from.node(depth);
    switch (node.type.name) {
      case "list_item":
        if (out.list === null) {
          out.list =
            node.attrs.checked != null
              ? "task"
              : $from.node(depth - 1).type.name === "ordered_list"
                ? "ordered"
                : "bullet";
        }
        break;
      case "blockquote":
        out.quote = true;
        break;
      case "table":
        out.table = true;
        break;
      case "table_header_row":
        out.tableHeader = true;
        break;
    }
  }
  const parent = $from.parent;
  if (parent.type.name === "heading") {
    const level = Math.min(6, Math.max(1, Number(parent.attrs.level) || 1));
    out.block = `h${level}` as BlockName;
  } else if (parent.type.name === "code_block") {
    out.block = "code";
    out.language = String(parent.attrs.language ?? "");
  } else if (parent.type.name === "paragraph") out.block = "paragraph";
  else out.block = "other";
  return out;
}

const HEADING_LEVEL: Record<string, number> = { h1: 1, h2: 2, h3: 3, h4: 4 };

/** Sets `checked` on every list item the selection touches. */
function setChecked(core: Core, value: boolean | null): void {
  const { view } = core;
  const { from, to } = view.state.selection;
  const tr = view.state.tr;
  view.state.doc.nodesBetween(from, to, (node: PMNode, pos: number) => {
    if (node.type.name === "list_item" && node.attrs.checked !== value) {
      tr.setNodeMarkup(pos, undefined, { ...node.attrs, checked: value });
    }
  });
  if (tr.docChanged) view.dispatch(tr);
}

/** Runs one of Milkdown's commands. */
function send<T>(core: Core, command: $Command<T>, payload?: T): boolean {
  return core.action((ctx) => ctx.get(commandsCtx).call(command.key, payload));
}

function liftOutOfList(core: Core): void {
  const { view } = core;
  for (let guard = 0; guard < 12 && formatState(view.state).list !== null; guard++) {
    const itemType = view.state.schema.nodes.list_item;
    if (!liftListItem(itemType)(view.state, view.dispatch)) break;
  }
}

function toggleList(core: Core, kind: "bullet" | "ordered" | "task"): void {
  const current = formatState(core.view.state).list;
  if (kind === "task") {
    if (current === "task") setChecked(core, null);
    else {
      if (current === null) send(core, wrapInBulletListCommand);
      setChecked(core, false);
    }
    return;
  }
  if (current === kind) {
    liftOutOfList(core);
    return;
  }
  if (current !== null) liftOutOfList(core);
  send(core, kind === "bullet" ? wrapInBulletListCommand : wrapInOrderedListCommand);
}

/**
 * A table the editor makes has every column aligned left, which it would write as `:-`. A table read from a file has no alignment
 * unless the file says so, and is written as `-`: the new one is made the same, so it looks like the tables around it in the file.
 */
function plainAlignment(core: Core): void {
  const { view } = core;
  const { $from } = view.state.selection;
  for (let depth = $from.depth; depth > 0; depth--) {
    if ($from.node(depth).type.name !== "table") continue;
    const start = $from.before(depth);
    const tr = view.state.tr;
    $from.node(depth).descendants((node, pos) => {
      if (node.attrs.alignment !== undefined && node.attrs.alignment !== null) {
        tr.setNodeMarkup(start + 1 + pos, undefined, { ...node.attrs, alignment: null });
      }
    });
    if (tr.docChanged) view.dispatch(tr);
    return;
  }
}

/** Runs a command of the table menu on the table the caret is in (the caller has checked that it can). */
function runTableCommand(core: Core, id: TableCommandId): boolean {
  const { view } = core;
  switch (id) {
    case "rowBefore":
      return send(core, addRowBeforeCommand);
    case "rowAfter":
      return send(core, addRowAfterCommand);
    case "colBefore":
      return send(core, addColBeforeCommand);
    case "colAfter":
      return send(core, addColAfterCommand);
    case "deleteRow":
      return deleteRow(view.state, view.dispatch);
    case "deleteCol":
      return deleteColumn(view.state, view.dispatch);
    case "deleteTable":
      return deleteTable(view.state, view.dispatch);
  }
}

/** Runs a toolbar command. Resolves to whether it did anything. */
export function runCommand(core: Core, id: CommandId): boolean {
  const { view } = core;
  const state = formatState(view.state);
  if (state.atom) return false;
  let done = true;
  switch (id) {
    case "bold":
      done = send(core, toggleStrongCommand);
      break;
    case "italic":
      done = send(core, toggleEmphasisCommand);
      break;
    case "strike":
      done = send(core, toggleStrikethroughCommand);
      break;
    case "code":
      done = send(core, toggleInlineCodeCommand);
      break;
    case "paragraph":
      done = send(core, turnIntoTextCommand);
      break;
    case "h1":
    case "h2":
    case "h3":
    case "h4":
      done =
        state.block === id
          ? send(core, turnIntoTextCommand)
          : send(core, wrapInHeadingCommand, HEADING_LEVEL[id]);
      break;
    case "bullet":
    case "ordered":
    case "task":
      toggleList(core, id);
      break;
    case "quote":
      if (state.quote) done = lift(view.state, view.dispatch);
      else done = send(core, wrapInBlockquoteCommand);
      break;
    case "codeBlock":
      done =
        state.block === "code"
          ? send(core, turnIntoTextCommand)
          : send(core, createCodeBlockCommand);
      break;
    case "rule":
      done = send(core, insertHrCommand);
      break;
    case "table":
      done = send(core, insertTableCommand, { row: 3, col: 3 });
      if (done) plainAlignment(core);
      break;
    case "rowBefore":
    case "rowAfter":
    case "colBefore":
    case "colAfter":
    case "deleteRow":
    case "deleteCol":
    case "deleteTable":
      done = tableUnavailable(state, id) === null && runTableCommand(core, id);
      break;
  }
  view.focus();
  return done;
}

/**
 * Sets the language of the code block the caret is in: the text goes into the fence's info string exactly as given (a name highlight.js
 * has no grammar for is fine; only line breaks and backticks, which an info string cannot hold, are left out).
 */
export function setCodeLanguage(core: Core, language: string): boolean {
  const { view } = core;
  const { $from } = view.state.selection;
  for (let depth = $from.depth; depth > 0; depth--) {
    if ($from.node(depth).type.name !== "code_block") continue;
    view.dispatch(
      view.state.tr.setNodeAttribute($from.before(depth), "language", sanitizeInfo(language)),
    );
    view.focus();
    return true;
  }
  return false;
}

/** Puts an image at the caret. */
export function insertImage(core: Core, src: string, alt = ""): boolean {
  const done = core.action((ctx) =>
    ctx.get(commandsCtx).call(insertImageCommand.key, { src, alt }),
  );
  core.view.focus();
  return done;
}
