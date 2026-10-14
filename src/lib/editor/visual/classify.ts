// Which blocks Visual mode may edit. A block is edited as a ProseMirror node only when the editor can write it back
// as the same Markdown; every other block becomes a locked atom that keeps its bytes. The rules, in this order:
//
//  1. By kind. Front matter, raw HTML, math, footnote definitions and `other` (alerts, description lists and anything
//     else the parser has no plain block for) are locked, and so is a Mermaid fence (a diagram, not text).
//  2. By content. A block with inline math (`$x$`) is locked: the editor has no math, and would read `$a_b_c$` as
//     emphasis and write it back changed.
//  3. By the editor's reading of it. The text of the block (line endings as LF, no final line break) is parsed by
//     Milkdown. It must become exactly one top-level node (`parse`, `multiple`).
//  4. By references. A block that uses a reference defined elsewhere in the document (`[text][ref]`, `[^note]`) is locked:
//     alone, the editor sees plain text there and would write it back escaped (`\[text]\[ref]`), cutting the link.
//  5. By equivalence. Let S be the block's text and R the Markdown the editor writes for the node it made from S.
//     The block is editable when R is the same text as S, or means the same thing (`sameMeaning`). Otherwise
//     (`differs`) the editor would lose or change something and the block is locked.
//
// "The same text" (`tidy`): line endings read as LF, and no line break at the end. Nothing else is touched: not the
// spaces at the end of a line, which can be a hard line break.
// "The same meaning": the two texts, read by the parser Milkdown reads with, give the same tree once the places in
// the text and the marker characters are left out (`core.mdast`). Emphasis written with `_` or `*`, a bullet written
// `*` or `-`, a table whose pipes are not aligned or a heading underlined with `===` read the same. A link title, the
// words after a code fence's language, a `<br>`, a footnote... that the editor would drop make the trees differ.
//
// An editable block that stays untouched is saved with its own bytes, so none of this changes a file by itself. A
// block that is edited is written in the editor's style (see `STRINGIFY_OPTIONS`), which rule 5 has shown to mean
// the same.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import type { BlockKind } from "../../types";
import { BOM, type BlockMap } from "../blockModel";
import type { Core, LockReason, MdastNode } from "./core";

/** Kinds the editor never edits. */
const LOCKED_KINDS: ReadonlySet<BlockKind> = new Set([
  "frontmatter",
  "html",
  "math",
  "footnoteDef",
  "other",
]);

/** A block of the Markdown source, as classification needs it. */
export type SourceBlock = { kind: BlockKind; text: string };

export type Verdict = { editable: true; node: PMNode } | { editable: false; reason: LockReason };

/** The text as it is compared: LF line endings, no line break at the end. */
export function tidy(text: string): string {
  return text.replace(/\r\n|\r/g, "\n").replace(/\n+$/, "");
}

/** A fenced code block whose language is `mermaid`. */
export function isDiagram(text: string): boolean {
  return /^ {0,3}(?:`{3,}|~{3,})[ \t]*mermaid(?:[ \t\r\n]|$)/i.test(text);
}

/** Text with something that looks like inline math: `$x$`, `$a_b$` (a `$` followed by no space, closed by one after no space). */
export function hasInlineMath(text: string): boolean {
  return /\$[^\s$](?:[^$\n]*[^\s$\\])?\$/.test(text);
}

const REFERENCE_TYPES = new Set(["linkReference", "imageReference", "footnoteReference"]);
/** A line that can define a reference: `[label]: target` or `[^note]: text`. */
const DEFINITION_LINE = /^ {0,3}\[[^\]\n]+\]:/m;

/** Whether two Markdown texts mean the same (see the header): the same text, or the same tree. */
export function sameMeaning(core: Core, source: string, written: string): boolean {
  if (tidy(source) === tidy(written)) return true;
  try {
    return JSON.stringify(core.mdast(tidy(source))) === JSON.stringify(core.mdast(tidy(written)));
  } catch {
    return false;
  }
}

/**
 * The blocks of a document that use a reference or footnote defined elsewhere. Found with the whole document read at once,
 * which is the only reading in which those references exist (and read as parsed: the editor's own reading turns a reference link
 * into an ordinary link and drops the definition). A document without anything that can define one is not read.
 */
export function blocksWithReferences(core: Core, map: BlockMap): Set<number> {
  const found = new Set<number>();
  if (map.blocks.length === 0) return found;
  const bom = map.seps[0]?.startsWith(BOM) ? BOM.length : 0;
  let text = map.seps[0].slice(bom);
  const ends: number[] = [];
  map.blocks.forEach((block, i) => {
    text += block.text;
    ends.push(text.length);
    text += map.seps[i + 1];
  });
  if (!DEFINITION_LINE.test(text)) return found;
  let tree: MdastNode;
  try {
    tree = core.parseRaw(text);
  } catch {
    // the document cannot be read as a whole: no reference can be told apart, so every block that mentions a bracket is held back
    map.blocks.forEach((block, i) => block.text.includes("[") && found.add(i));
    return found;
  }
  const blockAt = (offset: number): number => {
    let lo = 0;
    let hi = ends.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (ends[mid] > offset) hi = mid;
      else lo = mid + 1;
    }
    return lo;
  };
  const visit = (node: MdastNode): void => {
    if (REFERENCE_TYPES.has(node.type)) {
      const offset = node.position?.start.offset;
      if (offset !== undefined) found.add(blockAt(offset));
    }
    node.children?.forEach(visit);
  };
  visit(tree);
  return found;
}

/** Decides what one block of the source becomes. `usesReference` comes from `blocksWithReferences`. */
export function classifyBlock(core: Core, block: SourceBlock, usesReference: boolean): Verdict {
  if (LOCKED_KINDS.has(block.kind)) return { editable: false, reason: "kind" };
  if (block.kind === "code" && isDiagram(block.text)) return { editable: false, reason: "diagram" };
  if (hasInlineMath(block.text)) return { editable: false, reason: "math" };
  if (usesReference) return { editable: false, reason: "reference" };
  const source = tidy(block.text);
  if (source.trim() === "") return { editable: false, reason: "parse" };
  let doc: PMNode;
  try {
    doc = core.parse(source);
  } catch {
    return { editable: false, reason: "parse" };
  }
  if (doc.childCount !== 1) return { editable: false, reason: "parse" };
  const node = doc.child(0);
  let written: string;
  try {
    written = core.serialize(doc);
  } catch {
    return { editable: false, reason: "parse" };
  }
  if (!sameMeaning(core, source, written)) return { editable: false, reason: "differs" };
  return { editable: true, node };
}
