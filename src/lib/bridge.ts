// Imperative hooks that DocView registers for code that lives outside it (global shortcuts, the find
// bar, the selection popover). Components that need DOM access to the rendered document go through here.

import type { BlockMap } from "./editor/blockModel";
import type { VisualInput } from "./editor/visual/editor";
import type { FindMode } from "./find";

export type DocSelection = { text: string; section: string | null };

export type FindApi = {
  /** Runs a search. `near` positions the first match at or after that element (a located section). */
  run(
    query: string,
    mode: FindMode,
    opts?: { keepIndex?: boolean; near?: HTMLElement | null },
  ): void;
  step(direction: 1 | -1): void;
  clear(): void;
};

export type OutlineApi = {
  /** Scrolls a heading (by its index in the outline) to the top of the document and tints it briefly. */
  goTo(index: number): void;
  /** Puts the keyboard focus back on the document. */
  focusDoc(): void;
};

/** The Source editor, for code outside it (the edit store reads the text and moves the caret through this). */
export type SourceApi = {
  mode: "source";
  /** The text in the editor (no BOM, LF line endings). */
  getText(): string;
  /** Replaces the whole text and puts the caret at the start of a line (0-based). */
  setText(text: string, line: number): void;
  /** The line the caret is on (0-based). */
  caretLine(): number;
  focus(): void;
  /** Inserts the image `rel` (a path relative to the document) at the caret, replacing the selection. */
  insertImage(rel: string): void;
  /**
   * Replaces ranges of the text (UTF-16 offsets into the text as it is now, not overlapping) in one undoable step, and
   * leaves the caret where it was. The link rewrite of a move uses it on a document with unsaved edits.
   */
  applyChanges(edits: readonly { from: number; to: number; insert: string }[]): void;
};

/** What the store needs from Visual mode (the editor itself is `editor/visual/editor.ts`, loaded on first use). */
export type VisualApi = {
  mode: "visual";
  /** The file text a save would write (the BOM and line endings of the file, untouched blocks as they were). */
  text(): string;
  /** A save would write something other than the saved file. */
  status(): { dirty: boolean; changed: number };
  /** The line, in `text()`, of the block the caret is in (0-based). */
  caretLine(): number;
  /** The block of the loaded text the caret is in, or null in a block that was added. */
  caretBlock(): number | null;
  /** The line, in `text()`, where a block of the loaded text starts (null: the caret's block). */
  lineOfBlock(block: number | null): number;
  /** Replaces the document with the blocks of a text. */
  load(input: VisualInput): void;
  /** Starts over from the blocks of the text just saved: the unsaved-block bar clears, the selection stays. */
  rebase(map: BlockMap): void;
  focus(): void;
  insertImage(rel: string): void;
};

export type EditorApi = SourceApi | VisualApi;

export type DocBridge = {
  /** F7 / Shift+F7: move to the next or previous changed block or removed marker. */
  jumpChange?: (direction: 1 | -1) => void;
  find?: FindApi;
  /** The current text selection inside the document, with its nearest heading. */
  getSelection?: () => DocSelection | null;
  /** The element for a block or heading, used to position the first find match. */
  locatedElement?: () => HTMLElement | null;
  outline?: OutlineApi;
  /** The 1-based source line of the block at the top of the viewport, where editing starts. */
  topLine?: () => number | null;
  /** Set while the source editor is on screen. */
  editor?: EditorApi;
};

export const bridge: DocBridge = {};
