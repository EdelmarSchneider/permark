// What a Markdown text means, read by the parser the editor itself reads with: the checks of the Visual tests look at this, not at
// the characters, so that a save that changes the meaning cannot pass by writing the characters the test expected.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { mockParseBlocks } from "../../mock/mockBlocks";
import type { BlockKind } from "../../types";
import { LOCKED, type Core, type MdastNode } from "./core";

function walk(node: MdastNode, visit: (node: MdastNode) => void): void {
  visit(node);
  node.children?.forEach((child) => walk(child, visit));
}

/** What a text means, as the parser the editor reads with finds it. */
export type Meaning = {
  /** The link and footnote definitions, wherever they stand (a quote, a list item...), as `type:label`. */
  definitions: string[];
  /** The top-level blocks; a list is `list:bullet` or `list:ordered`. */
  blocks: string[];
};

export function meaningOf(core: Core, text: string): Meaning {
  const tree = core.parseRaw(text);
  const definitions: string[] = [];
  walk(tree, (node) => {
    if (node.type === "definition" || node.type === "footnoteDefinition") {
      definitions.push(`${node.type}:${String(node.identifier)}`);
    }
  });
  const blocks = (tree.children ?? []).map((node) =>
    node.type === "list" ? (node.ordered === true ? "list:ordered" : "list:bullet") : node.type,
  );
  return { definitions: definitions.sort(), blocks };
}

export const definitionsOf = (core: Core, text: string): string[] =>
  meaningOf(core, text).definitions;
export const topLevel = (core: Core, text: string): string[] => meaningOf(core, text).blocks;

/** The kinds of the blocks the mock splitter finds in a text (it can read a saved text again, which the recorded ranges cannot). */
export function splitKinds(text: string): BlockKind[] {
  return mockParseBlocks(text).blocks.map((b) => b.kind);
}

/** The kind of block a top-level node is saved as, or null for a node this does not know. */
export function kindOfNode(node: PMNode): BlockKind | null {
  switch (node.type.name) {
    case LOCKED:
      return node.attrs.kind as BlockKind;
    case "paragraph":
      return "paragraph";
    case "heading":
      return "heading";
    case "bullet_list":
    case "ordered_list":
      return "list";
    case "code_block":
      return "code";
    case "blockquote":
      return "blockquote";
    case "hr":
      return "thematicBreak";
    case "table":
      return "table";
    default:
      return null;
  }
}
