// What the toolbar and the menus of Visual mode know about the selection, and the names of the commands they run. Kept apart from
// the editor so that the UI can use them without loading Milkdown.

/** The block the caret is in, as the toolbar names it. */
export type BlockName = "paragraph" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "code" | "other";

/** What the toolbar shows for the selection. */
export type FormatState = {
  block: BlockName;
  bold: boolean;
  italic: boolean;
  strike: boolean;
  code: boolean;
  link: boolean;
  list: "bullet" | "ordered" | "task" | null;
  quote: boolean;
  table: boolean;
  /** The caret is in the header row of a table, which cannot get a row above it or be deleted (a table needs its header). */
  tableHeader: boolean;
  /** The selection is a locked atom: nothing can be formatted there. */
  atom: boolean;
  /** The language of the code block the caret is in. */
  language: string;
};

export const NO_FORMAT: FormatState = {
  block: "paragraph",
  bold: false,
  italic: false,
  strike: false,
  code: false,
  link: false,
  list: null,
  quote: false,
  table: false,
  tableHeader: false,
  atom: false,
  language: "",
};

/** The commands of the table menu that work on the table the caret is in. */
export type TableCommandId =
  "rowBefore" | "rowAfter" | "colBefore" | "colAfter" | "deleteRow" | "deleteCol" | "deleteTable";

/** Why a table command cannot run: the caret is not in a table, or it is in the header row, which stays the first row. */
export type TableBlock = "notInTable" | "headerRowAbove" | "headerRowDelete";

/**
 * Whether a table command can run for the selection, and when it cannot, why. The menus show this as the reason an entry is off;
 * the commands refuse in the same cases. A row below the header row is allowed: it is the first row of the table.
 */
export function tableUnavailable(format: FormatState, id: TableCommandId): TableBlock | null {
  if (!format.table) return "notInTable";
  if (format.tableHeader && id === "rowBefore") return "headerRowAbove";
  if (format.tableHeader && id === "deleteRow") return "headerRowDelete";
  return null;
}

export type CommandId =
  | "bold"
  | "italic"
  | "strike"
  | "code"
  | "paragraph"
  | "h1"
  | "h2"
  | "h3"
  | "h4"
  | "bullet"
  | "ordered"
  | "task"
  | "quote"
  | "codeBlock"
  | "rule"
  | "table"
  | "rowBefore"
  | "rowAfter"
  | "colBefore"
  | "colAfter"
  | "deleteRow"
  | "deleteCol"
  | "deleteTable";
