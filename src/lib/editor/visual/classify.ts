// Which blocks Visual mode may edit. A block is edited as a ProseMirror node only when the editor can write it back
// as the same Markdown; every other block becomes a locked atom that keeps its bytes. The rules, in this order:
//
//  1. By kind. Front matter, raw HTML, math, footnote definitions and `other` (alerts, description lists and anything
//     else the parser has no plain block for) are locked, and so is a Mermaid fence (a diagram, not text).
//  2. By content. A block with inline math (`$x$`) is locked: the editor has no math, and would read `$a_b_c$` as
//     emphasis and write it back changed. So is a block that starts with a byte order mark (not the file's own), which the
//     parser would drop.
//  3. By the editor's reading of it. The text of the block (line endings as LF, no final line break) is parsed by
//     Milkdown. It must become exactly one top-level node (`parse`, `multiple`).
//  4. By references. A block that holds the definition of a link reference or a footnote (`[ref]: url`, also inside a quote or a
//     list item) is locked: the editor resolves references while it reads, so the definition is not in its document and
//     would be gone from the file once the block is written. So is a block that uses a reference defined elsewhere in the
//     document (`[text][ref]`, `[^note]`): alone, the editor sees plain text there and would write it back escaped
//     (`\[text]\[ref]`), cutting the link.
//  5. By equivalence. Let S be the block's text and R the Markdown the editor writes for the node it made from S.
//     The block is editable when R is the same text as S, or means the same thing (`sameMeaning`). Otherwise
//     (`differs`) the editor would lose or change something and the block is locked.
//
// "The same text" (`tidy`): line endings read as LF, and no line break at the end. Nothing else is touched: not the
// spaces at the end of a line, which can be a hard line break.
// "The same meaning": the two texts, read by the parser Milkdown reads with, give the same tree once the places in
// the text and the marker characters are left out (`core.mdast`). Emphasis written with `_` or `*`, a bullet written
// `*` or `-`, a table whose pipes are not aligned or a heading underlined with `===` read the same. A link title, the
// words after a code fence's language, a footnote... that the editor would drop make the trees differ. (A `<br>` does not:
// the editor reads it as a line break and writes it back as `<br>`.)
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
const DEFINITION_TYPES = new Set(["definition", "footnoteDefinition"]);
/**
 * Every definition has `]:` right after its label, wherever it stands (a line of its own, in a quote, in a list item), so a text
 * without that has none. Looking for a line that starts with a label would miss the ones inside other blocks.
 */
const MAY_DEFINE = "]:";

/** Whether two Markdown texts mean the same (see the header): the same text, or the same tree. */
export function sameMeaning(core: Core, source: string, written: string): boolean {
  if (tidy(source) === tidy(written)) return true;
  try {
    return JSON.stringify(core.mdast(tidy(source))) === JSON.stringify(core.mdast(tidy(written)));
  } catch {
    return false;
  }
}

/** Whether a tree holds the definition of a link reference or a footnote anywhere in it. */
function holdsDefinition(node: MdastNode): boolean {
  return DEFINITION_TYPES.has(node.type) || (node.children?.some(holdsDefinition) ?? false);
}

/** A definition as it can be written on a line of its own, whatever it stood in. */
function definitionText(node: MdastNode, source: string): string {
  if (node.type === "definition") {
    const label = typeof node.label === "string" ? node.label : String(node.identifier);
    const title =
      typeof node.title === "string" ? ` "${node.title.replace(/["\\]/g, "\\$&")}"` : "";
    return `[${label}]: <${String(node.url)}>${title}`;
  }
  const start = node.position?.start.offset;
  const end = node.position?.end.offset;
  return start === undefined || end === undefined ? "" : source.slice(start, end);
}

/**
 * The blocks of a document that use a reference or footnote defined elsewhere, each with the definitions it uses (as lines of
 * Markdown, for drawing the block beside them). Found with the whole document read at once, which is the only reading in which
 * those references exist (and read as parsed: the editor's own reading turns a reference link into an ordinary link and drops
 * the definition). A document without anything that can define one is not read.
 */
export function referenceUses(core: Core, map: BlockMap): Map<number, string> {
  const found = new Map<number, string>();
  if (map.blocks.length === 0) return found;
  const bom = map.seps[0]?.startsWith(BOM) ? BOM.length : 0;
  let text = map.seps[0].slice(bom);
  const ends: number[] = [];
  map.blocks.forEach((block, i) => {
    text += block.text;
    ends.push(text.length);
    text += map.seps[i + 1];
  });
  if (!text.includes(MAY_DEFINE)) return found;
  let tree: MdastNode;
  try {
    tree = core.parseRaw(text);
  } catch {
    // the document cannot be read as a whole: no reference can be told apart, so every block that mentions a bracket is held back
    map.blocks.forEach((block, i) => block.text.includes("[") && found.set(i, ""));
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
  const defined = new Map<string, string>();
  const used = new Map<number, Set<string>>();
  const visit = (node: MdastNode): void => {
    if (DEFINITION_TYPES.has(node.type)) {
      const line = definitionText(node, text);
      if (line !== "") defined.set(`${node.type}:${String(node.identifier)}`, line);
    } else if (REFERENCE_TYPES.has(node.type)) {
      const offset = node.position?.start.offset;
      if (offset !== undefined) {
        const block = blockAt(offset);
        const kind = node.type === "footnoteReference" ? "footnoteDefinition" : "definition";
        const wanted = used.get(block) ?? new Set<string>();
        wanted.add(`${kind}:${String(node.identifier)}`);
        used.set(block, wanted);
      }
    }
    node.children?.forEach(visit);
  };
  visit(tree);
  for (const [block, wanted] of used) {
    found.set(
      block,
      [...wanted]
        .map((key) => defined.get(key))
        .filter((line) => line !== undefined)
        .join("\n\n"),
    );
  }
  return found;
}

/** The blocks of a document that use a reference or footnote defined elsewhere. */
export function blocksWithReferences(core: Core, map: BlockMap): Set<number> {
  return new Set(referenceUses(core, map).keys());
}

/** Whether the block, read on its own as the parser reads it, holds the definition of a reference (a block it cannot read does not). */
function definesReference(core: Core, source: string): boolean {
  try {
    return holdsDefinition(core.parseRaw(source));
  } catch {
    return false;
  }
}

/** Decides what one block of the source becomes. `usesReference` comes from `blocksWithReferences`. */
export function classifyBlock(core: Core, block: SourceBlock, usesReference: boolean): Verdict {
  if (LOCKED_KINDS.has(block.kind)) return { editable: false, reason: "kind" };
  if (block.kind === "code" && isDiagram(block.text)) return { editable: false, reason: "diagram" };
  if (hasInlineMath(block.text)) return { editable: false, reason: "math" };
  // the parser takes a byte order mark at the start of its input for the file's own and drops it: the block would be read
  // without a character it has, and written without it
  if (block.text.startsWith(BOM)) return { editable: false, reason: "parse" };
  if (block.text.includes(MAY_DEFINE) && definesReference(core, tidy(block.text))) {
    return { editable: false, reason: "definition" };
  }
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
