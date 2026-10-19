// What the core hands to the views plugins contribute (types only; the registry and the components share them).

import type { DocumentView, WorkspaceId } from "../../api";

/** An embed (a code block, a formula, a diagram) is mounted with the text it draws. */
export type EmbedProps = {
  /** The text: code, formula, diagram source. */
  source: string;
  /** The word after the fence (`rust`, `mermaid`); "" when there is none. */
  info: string;
  /** A formula on a line of its own. */
  display: boolean;
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
