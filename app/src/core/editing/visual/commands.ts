// Edit-mode commands (toolbar, slash menu, keys) over the Milkdown core, and what the toolbar shows for the
// selection. Nothing runs inside a locked block.

import {
  createCodeBlockCommand,
  insertHrCommand,
  insertImageCommand,
  liftListItemCommand,
  sinkListItemCommand,
  toggleEmphasisCommand,
  toggleInlineCodeCommand,
  toggleLinkCommand,
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
import type { MarkType, Node as PMNode, ResolvedPos } from "@milkdown/kit/prose/model";
import { NodeSelection, type EditorState, type Transaction } from "@milkdown/kit/prose/state";
import { deleteColumn, deleteRow, deleteTable } from "@milkdown/kit/prose/tables";
import type { CmdKey } from "@milkdown/kit/core";
import { callCommand } from "@milkdown/kit/utils";
import { LOCKED, type Core } from "./core";

export type BlockType = "paragraph" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "code" | "other";

export type Format = {
  block: BlockType;
  strong: boolean;
  emphasis: boolean;
  strike: boolean;
  code: boolean;
  link: string | null;
  list: "bullet" | "ordered" | "task" | null;
  table: boolean;
  /** The caret is in the header row of a table (no row can go above it). */
  tableHeader: boolean;
  /** The selection is on a locked block: nothing to do there. */
  locked: boolean;
  /** Language of the code block the caret is in. */
  language: string | null;
};

function markActive(state: EditorState, type: MarkType | undefined): boolean {
  if (!type) return false;
  const { from, $from, to, empty } = state.selection;
  if (empty) return type.isInSet(state.storedMarks ?? $from.marks()) !== undefined;
  return state.doc.rangeHasMark(from, to, type);
}

function ancestor($pos: ResolvedPos, name: string): PMNode | null {
  for (let d = $pos.depth; d > 0; d--) {
    const node = $pos.node(d);
    if (node.type.name === name) return node;
  }
  return null;
}

const HEADINGS: readonly BlockType[] = ["h1", "h2", "h3", "h4", "h5", "h6"];

export function formatOf(state: EditorState): Format {
  const { $from } = state.selection;
  const schema = state.schema;
  const parent = $from.parent;
  const selected = state.selection instanceof NodeSelection ? state.selection.node : undefined;
  let block: BlockType = "other";
  if (parent.type.name === "paragraph") block = "paragraph";
  else if (parent.type.name === "heading") {
    block = HEADINGS[Number(parent.attrs.level) - 1] ?? "other";
  } else if (parent.type.name === "code_block") block = "code";
  const item = ancestor($from, "list_item");
  const bullet = ancestor($from, "bullet_list");
  const ordered = ancestor($from, "ordered_list");
  let list: Format["list"] = null;
  if (item && item.attrs.checked !== null && item.attrs.checked !== undefined) list = "task";
  else if (ordered && (!bullet || $from.depth > 0)) list = bullet ? "bullet" : "ordered";
  else if (bullet) list = "bullet";
  const linkMark = schema.marks.link;
  // a selection: the marks of its first character (a link does not reach out to the position before it)
  const first = state.selection.empty ? null : state.doc.nodeAt(state.selection.from);
  const marks = first?.marks ?? $from.marks();
  const href: unknown = linkMark ? marks.find((m) => m.type === linkMark)?.attrs.href : undefined;
  const link = typeof href === "string" ? href : undefined;
  return {
    block,
    strong: markActive(state, schema.marks.strong),
    emphasis: markActive(state, schema.marks.emphasis),
    strike: markActive(state, schema.marks.strike_through),
    code: markActive(state, schema.marks.inlineCode),
    link: link ?? null,
    list,
    table: ancestor($from, "table") !== null,
    tableHeader: ancestor($from, "table_header_row") !== null,
    locked: selected?.type.name === LOCKED,
    language: block === "code" ? String(parent.attrs.language ?? "") : null,
  };
}

export type Command =
  | { kind: "block"; to: BlockType }
  | { kind: "strong" | "emphasis" | "strike" | "code" }
  | { kind: "link"; href: string | null }
  | { kind: "bulletList" | "orderedList" | "taskList" }
  | { kind: "indent" | "outdent" }
  | { kind: "quote" | "rule" | "codeBlock" }
  | { kind: "language"; language: string }
  | { kind: "table" | "addRow" | "addRowAbove" | "addColumn" | "addColumnLeft" }
  | { kind: "deleteRow" | "deleteColumn" | "deleteTable" }
  | { kind: "image"; src: string; alt: string };

function act<T>(core: Core, key: CmdKey<T>, payload?: T): boolean {
  return core.editor.action(callCommand(key, payload));
}

/** Runs a command; false when it could not apply here. */
export function run(core: Core, command: Command): boolean {
  const view = core.view;
  if (formatOf(view.state).locked) return false;
  switch (command.kind) {
    case "block": {
      if (command.to === "paragraph") return act(core, turnIntoTextCommand.key);
      if (command.to === "code") return act(core, createCodeBlockCommand.key, "");
      if (command.to === "other") return false;
      const level = Number(command.to.slice(1));
      return act(core, wrapInHeadingCommand.key, level);
    }
    case "strong":
      return act(core, toggleStrongCommand.key);
    case "emphasis":
      return act(core, toggleEmphasisCommand.key);
    case "strike":
      return act(core, toggleStrikethroughCommand.key);
    case "code":
      return act(core, toggleInlineCodeCommand.key);
    case "link":
      return setLink(core, command.href);
    case "bulletList":
      return liftThenWrap(core, "bullet_list") || act(core, wrapInBulletListCommand.key);
    case "orderedList":
      return liftThenWrap(core, "ordered_list") || act(core, wrapInOrderedListCommand.key);
    case "taskList":
      return toggleTask(core);
    case "indent":
      return act(core, sinkListItemCommand.key);
    case "outdent":
      return act(core, liftListItemCommand.key);
    case "quote":
      return act(core, wrapInBlockquoteCommand.key);
    case "rule":
      return act(core, insertHrCommand.key);
    case "codeBlock":
      return act(core, createCodeBlockCommand.key, "");
    case "language": {
      const { $from } = view.state.selection;
      if ($from.parent.type.name !== "code_block") return false;
      const block = $from.parent;
      view.dispatch(
        view.state.tr.setNodeMarkup($from.before(), undefined, {
          ...block.attrs,
          language: command.language.trim(),
        }),
      );
      return true;
    }
    case "table":
      return act(core, insertTableCommand.key, { row: 3, col: 3 });
    case "addRow":
      return act(core, addRowAfterCommand.key);
    case "addRowAbove":
      // the header row stays first: a GFM table has no row above it
      if (formatOf(view.state).tableHeader) return false;
      return act(core, addRowBeforeCommand.key);
    case "addColumn":
      return act(core, addColAfterCommand.key);
    case "addColumnLeft":
      return act(core, addColBeforeCommand.key);
    case "deleteRow":
      return deleteRow(view.state, view.dispatch);
    case "deleteColumn":
      return deleteColumn(view.state, view.dispatch);
    case "deleteTable":
      return deleteTable(view.state, view.dispatch);
    case "image":
      return act(core, insertImageCommand.key, { src: command.src, alt: command.alt, title: "" });
    default:
      return command satisfies never;
  }
}

/** A list of the other kind around the caret becomes this kind (bullet <-> ordered). */
function liftThenWrap(core: Core, to: "bullet_list" | "ordered_list"): boolean {
  const view = core.view;
  const { $from } = view.state.selection;
  for (let d = $from.depth; d > 0; d--) {
    const node = $from.node(d);
    const name = node.type.name;
    if (name === to) return false;
    if (name === "bullet_list" || name === "ordered_list") {
      const type = view.state.schema.nodes[to];
      if (!type) return false;
      view.dispatch(view.state.tr.setNodeMarkup($from.before(d), type, {}));
      return true;
    }
  }
  return false;
}

/** Items of the list around the caret become tasks (unchecked), or plain items again. */
function toggleTask(core: Core): boolean {
  const view = core.view;
  const { $from } = view.state.selection;
  let listDepth = -1;
  for (let d = $from.depth; d > 0; d--) {
    const name = $from.node(d).type.name;
    if (name === "bullet_list" || name === "ordered_list") {
      listDepth = d;
      break;
    }
  }
  if (listDepth < 0) {
    if (!act(core, wrapInBulletListCommand.key)) return false;
    return toggleTask(core);
  }
  const list = $from.node(listDepth);
  const start = $from.before(listDepth);
  const anyTask = (() => {
    let found = false;
    list.forEach((item) => {
      if (item.attrs.checked !== null && item.attrs.checked !== undefined) found = true;
    });
    return found;
  })();
  let tr: Transaction = view.state.tr;
  list.forEach((item, offset) => {
    tr = tr.setNodeMarkup(start + 1 + offset, undefined, {
      ...item.attrs,
      checked: anyTask ? null : false,
    });
  });
  view.dispatch(tr);
  return true;
}

function setLink(core: Core, href: string | null): boolean {
  const view = core.view;
  const linkType = view.state.schema.marks.link;
  if (!linkType) return false;
  const { from, to, empty, $from } = view.state.selection;
  if (href === null) {
    // the whole link around the caret goes
    const range = markRange($from, linkType);
    const [a, b] = empty && range ? range : [from, to];
    view.dispatch(view.state.tr.removeMark(a, b, linkType));
    return true;
  }
  if (empty) {
    const range = markRange($from, linkType);
    if (range) {
      view.dispatch(view.state.tr.addMark(range[0], range[1], linkType.create({ href })));
      return true;
    }
    const text = view.state.schema.text(href, [linkType.create({ href })]);
    view.dispatch(view.state.tr.replaceSelectionWith(text, false));
    return true;
  }
  return act(core, toggleLinkCommand.key, { href });
}

/** The extent of `type` around a position (inside one text block). */
function markRange($pos: ResolvedPos, type: MarkType): [number, number] | null {
  const parent = $pos.parent;
  const start = $pos.start();
  let found: [number, number] | null = null;
  let at = 0;
  parent.forEach((child) => {
    const from = start + at;
    const to = from + child.nodeSize;
    at += child.nodeSize;
    if (!type.isInSet(child.marks)) {
      if (found && found[1] < $pos.pos) found = null;
      return;
    }
    if (found && found[1] === from) found = [found[0], to];
    else if (!found || found[1] < $pos.pos) found = [from, to];
  });
  return found && found[0] <= $pos.pos && $pos.pos <= found[1] ? found : null;
}
