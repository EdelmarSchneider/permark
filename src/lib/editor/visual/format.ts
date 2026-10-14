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
