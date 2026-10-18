// The toolbar of Visual mode as data (docs/design/v0.2.md section 3): which buttons there are, in which order, how they show the
// state of the selection, and how many fit before the rest folds into the `[...]` menu. The component only draws this.

import type { IconName } from "../../components/Icon.svelte";
import type { MenuEntry } from "../../components/Menu.svelte";
import type { MessageKey } from "../../i18n";
import {
  tableUnavailable,
  type BlockName,
  type CommandId,
  type FormatState,
  type TableBlock,
  type TableCommandId,
} from "./format";

export type ToolbarItem = {
  id: CommandId | "link" | "image";
  label: MessageKey;
  icon: IconName;
  /** The shortcut shown in the tooltip (Ctrl on Windows and Linux). */
  hint?: string;
  /** Pressed: the selection has this format. */
  active(format: FormatState): boolean;
  /** A space is left before it (it starts a group of buttons). */
  group: boolean;
  /** Width on screen in px, with the gap before it; the toolbar's room is shared out in these. */
  width: number;
};

const BUTTON = 30;
const GAP_BEFORE_GROUP = 8;

const item = (
  id: ToolbarItem["id"],
  label: MessageKey,
  icon: IconName,
  hint: string | undefined,
  active: ToolbarItem["active"],
  startsGroup = false,
): ToolbarItem => ({
  id,
  label,
  icon,
  hint,
  active,
  group: startsGroup,
  width: BUTTON + (startsGroup ? GAP_BEFORE_GROUP : 0),
});

/** The buttons after the block type, in order. The table menu sits between the quote and the code block (see `TABLE_WIDTH`). */
export const TOOLBAR_ITEMS: readonly ToolbarItem[] = [
  item("bold", "toolbar.bold", "bold", "Ctrl+B", (f) => f.bold, true),
  item("italic", "toolbar.italic", "italic", "Ctrl+I", (f) => f.italic),
  item("strike", "toolbar.strike", "strike", "Ctrl+Shift+X", (f) => f.strike),
  item("code", "toolbar.code", "code", "Ctrl+`", (f) => f.code),
  item("link", "toolbar.link", "link", "Ctrl+K", (f) => f.link, true),
  item("bullet", "toolbar.bulletList", "list", undefined, (f) => f.list === "bullet", true),
  item("ordered", "toolbar.orderedList", "list-ordered", undefined, (f) => f.list === "ordered"),
  item("task", "toolbar.taskList", "list-checks", undefined, (f) => f.list === "task"),
  item("quote", "toolbar.quote", "quote", "Ctrl+Shift+.", (f) => f.quote, true),
  item("table", "toolbar.table", "table", undefined, (f) => f.table, true),
  item("codeBlock", "toolbar.codeBlock", "square-code", "Ctrl+Alt+C", (f) => f.block === "code"),
  item("image", "toolbar.image", "image", undefined, () => false),
  item("rule", "toolbar.rule", "minus", undefined, () => false),
];

/** Room taken by the block type dropdown and by the `[...]` button. */
export const BLOCK_TYPE_WIDTH = 132;
export const MORE_WIDTH = BUTTON + GAP_BEFORE_GROUP;
/** Room taken by the language button while the caret is in a code block. */
export const LANGUAGE_WIDTH = 108;
/** Side padding of the toolbar. */
export const TOOLBAR_PADDING = 16;

/**
 * How many of the items fit in a toolbar `available` px wide. When some do not, the last ones fold into the `[...]` menu, whose
 * button takes room too. At least none, at most all.
 */
export function visibleCount(widths: readonly number[], available: number): number {
  const room = available - TOOLBAR_PADDING - BLOCK_TYPE_WIDTH;
  const total = widths.reduce((sum, w) => sum + w, 0);
  if (total <= room) return widths.length;
  let used = MORE_WIDTH;
  let count = 0;
  for (const width of widths) {
    if (used + width > room) break;
    used += width;
    count++;
  }
  return count;
}

/** The sentence that says why a table command is off. */
const TABLE_WHY: Record<TableBlock, MessageKey> = {
  notInTable: "toolbar.tableNeedsCell",
  headerRowAbove: "toolbar.tableHeaderNoRowAbove",
  headerRowDelete: "toolbar.tableHeaderNoDelete",
};

function tableEntry(
  format: FormatState,
  id: TableCommandId,
  label: MessageKey,
  labelOf: (key: MessageKey) => string,
  run: (id: CommandId) => void,
  icon?: IconName,
  initial?: boolean,
): MenuEntry {
  const why = tableUnavailable(format, id);
  return {
    id,
    label: labelOf(label),
    icon,
    disabled: why !== null,
    title: why === null ? undefined : labelOf(TABLE_WHY[why]),
    initial,
    onselect: () => run(id),
  };
}

/**
 * The entries of the table menu: insert a table, and, when the caret is in one, add and delete its rows and columns. An entry that
 * is off says why, as a tooltip; when the caret is not in a table at all, so does a line of text in the menu (a keyboard has no tooltips).
 * In a table the keyboard starts on a row action: "Insert table" first would make Enter put a second table there.
 */
export function tableMenuEntries(
  format: FormatState,
  labelOf: (key: MessageKey) => string,
  run: (id: CommandId) => void,
): MenuEntry[] {
  const first = (["rowBefore", "rowAfter"] as const).find(
    (id) => tableUnavailable(format, id) === null,
  );
  const entry = (id: TableCommandId, label: MessageKey, icon?: IconName): MenuEntry =>
    tableEntry(format, id, label, labelOf, run, icon, id === first);
  return [
    {
      id: "insert",
      label: labelOf("toolbar.insertTable"),
      icon: "table",
      onselect: () => run("table"),
    },
    { type: "separator" },
    ...(format.table ? [] : [{ type: "note", text: labelOf(TABLE_WHY.notInTable) } as const]),
    entry("rowBefore", "toolbar.rowAbove"),
    entry("rowAfter", "toolbar.rowBelow"),
    entry("colBefore", "toolbar.colLeft"),
    entry("colAfter", "toolbar.colRight"),
    { type: "separator" },
    entry("deleteRow", "toolbar.deleteRow"),
    entry("deleteCol", "toolbar.deleteCol"),
    entry("deleteTable", "toolbar.deleteTable", "trash"),
  ];
}

/**
 * The `[...]` menu: the buttons that did not fit in the toolbar. The table button is a menu of its own, so when it is one of them its
 * entries are listed here (a single "Table" entry would only insert a table, and leave no way to add or delete a row or a column).
 * `act` runs a button, `run` a table command; `off` turns every entry off (a locked block, an editor that is still loading).
 */
export function moreMenuEntries(
  folded: readonly ToolbarItem[],
  format: FormatState,
  off: boolean,
  labelOf: (key: MessageKey) => string,
  act: (item: ToolbarItem) => void,
  run: (id: CommandId) => void,
): MenuEntry[] {
  const out: MenuEntry[] = [];
  folded.forEach((item, i) => {
    if (item.id !== "table") {
      out.push({
        id: item.id,
        label: labelOf(item.label),
        icon: item.icon,
        hint: item.hint,
        checked: item.id === "link" || item.id === "image" ? undefined : item.active(format),
        disabled: off,
        onselect: () => act(item),
      });
      return;
    }
    if (i > 0) out.push({ type: "separator" });
    out.push({ type: "label", label: labelOf(item.label) });
    for (const entry of tableMenuEntries(format, labelOf, run)) {
      out.push(
        off && entry.type !== "separator" && entry.type !== "label" && entry.type !== "note"
          ? { ...entry, disabled: true }
          : entry,
      );
    }
    if (i < folded.length - 1) out.push({ type: "separator" });
  });
  return out;
}

/** The block types of the dropdown (and the slash menu), in order. */
export const BLOCK_TYPES: readonly {
  id: CommandId;
  block: BlockName;
  label: MessageKey;
  hint: string;
}[] = [
  { id: "paragraph", block: "paragraph", label: "toolbar.paragraph", hint: "Ctrl+Shift+0" },
  { id: "h1", block: "h1", label: "toolbar.heading1", hint: "Ctrl+Shift+1" },
  { id: "h2", block: "h2", label: "toolbar.heading2", hint: "Ctrl+Shift+2" },
  { id: "h3", block: "h3", label: "toolbar.heading3", hint: "Ctrl+Shift+3" },
  { id: "h4", block: "h4", label: "toolbar.heading4", hint: "Ctrl+Shift+4" },
  { id: "h5", block: "h5", label: "toolbar.heading5", hint: "Ctrl+Shift+5" },
  { id: "h6", block: "h6", label: "toolbar.heading6", hint: "Ctrl+Shift+6" },
];

/** The label of the block the caret is in, for the dropdown's button. */
export function blockLabel(block: BlockName): MessageKey {
  switch (block) {
    case "h1":
      return "toolbar.heading1";
    case "h2":
      return "toolbar.heading2";
    case "h3":
      return "toolbar.heading3";
    case "h4":
      return "toolbar.heading4";
    case "h5":
      return "toolbar.heading5";
    case "h6":
      return "toolbar.heading6";
    case "code":
      return "toolbar.codeBlockType";
    default:
      return "toolbar.paragraph";
  }
}

/** What the slash menu offers, in order. */
export type SlashItem = {
  id: CommandId | "image";
  label: MessageKey;
  icon: IconName;
  /** Extra words the filter matches besides the label. */
  keywords: string;
};

export const SLASH_ITEMS: readonly SlashItem[] = [
  { id: "paragraph", label: "toolbar.paragraph", icon: "pilcrow", keywords: "text plain body" },
  { id: "h1", label: "toolbar.heading1", icon: "heading-1", keywords: "heading title h1" },
  { id: "h2", label: "toolbar.heading2", icon: "heading-2", keywords: "heading h2" },
  { id: "h3", label: "toolbar.heading3", icon: "heading-3", keywords: "heading h3" },
  { id: "h4", label: "toolbar.heading4", icon: "heading-4", keywords: "heading h4" },
  { id: "bullet", label: "toolbar.bulletList", icon: "list", keywords: "list unordered ul" },
  { id: "ordered", label: "toolbar.orderedList", icon: "list-ordered", keywords: "list ol number" },
  { id: "task", label: "toolbar.taskList", icon: "list-checks", keywords: "todo checkbox check" },
  { id: "quote", label: "toolbar.quote", icon: "quote", keywords: "blockquote callout" },
  {
    id: "codeBlock",
    label: "toolbar.codeBlock",
    icon: "square-code",
    keywords: "code snippet fence",
  },
  { id: "table", label: "toolbar.table", icon: "table", keywords: "grid rows columns" },
  { id: "rule", label: "toolbar.rule", icon: "minus", keywords: "divider line hr separator" },
  { id: "image", label: "toolbar.image", icon: "image", keywords: "picture photo file" },
];

/** The slash items whose label or keywords contain every word typed after the `/` (all of them for an empty query). */
export function filterSlash(
  items: readonly SlashItem[],
  query: string,
  labelOf: (key: MessageKey) => string,
): SlashItem[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [...items];
  return items.filter((entry) => {
    const haystack = `${labelOf(entry.label)} ${entry.keywords}`.toLowerCase();
    return words.every((word) => haystack.includes(word));
  });
}
