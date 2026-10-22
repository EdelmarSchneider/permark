// What the core hands to the views plugins contribute (types only; the registry and the components share them).

import type { DocumentView, OutlineItem, WorkspaceId, WorkspaceView } from "../../api";

/** An embed (a code block, a formula, a diagram) is mounted with the text it draws. */
export type EmbedProps = {
  /** The text: code, formula, diagram source. */
  source: string;
  /** The word after the fence (`rust`, `mermaid`); "" when there is none. */
  info: string;
  /** A formula on a line of its own. */
  display: boolean;
  /** Code: each line in an element of its own (`span.ln`), for a viewer that wraps long lines and numbers them. */
  lines?: boolean;
  /** Code: the text scrolls sideways in a box the keyboard reaches (a viewer of a whole file), named this. */
  region?: string;
};

/** The text selected in the page, and where it is (a plugin comments on it). */
export type PageSelection = {
  /** The selected text as the reader shows it. */
  text: string;
  /** The node the selection starts in (the reference and the block lines are found from it). */
  node: Node | null;
  /** The node it ends in. */
  endNode: Node | null;
  /** Where it is on screen. */
  rect: DOMRect | null;
};

/** A viewer shows the body of a document of its format (text, a picture, ...). */
export type ViewerProps = {
  view: DocumentView;
  workspace: WorkspaceId;
};

/** The card of the front matter of a Markdown document. */
export type FrontMatterProps = {
  syntax: "yaml" | "toml";
  raw: string;
  fields: readonly {
    key: string;
    value: { type: "text"; text: string } | { type: "list"; items: string[] };
  }[];
};

/** The page a plugin gives a workspace of its own kind (the Notes window). */
export type WorkspaceViewProps = {
  /** This window's workspace. */
  workspace: Extract<WorkspaceView, { kind: "notes" }>;
};

/** The headings the outline panel shows: a plugin that adds a column to them works out its figures from these. */
export type OutlineWatcherProps = {
  items: readonly OutlineItem[];
};
