// Which blocks Edit mode may change. A block becomes an editable node only when the editor writes it back as the
// same Markdown; else it is a locked atom keeping its bytes. In order:
//  1. backend kind (front matter, HTML, math, footnotes, diagrams, definitions, alerts, ...) - `segment.lock`;
//  2. content: a BOM at the start (parser drops it);
//  3. references: a block holding a link | footnote definition (the editor resolves and drops it), or a picture by
//     reference defined elsewhere (the editor has no piece for it: alone, it reads as text and is written escaped);
//     links and footnotes by reference are inline pieces kept as written (`references.ts`);
//  4. the editor's reading: exactly one top-level node, no inline piece across lines (a piece is one thing of one line
//     in the editor; container prefixes inside it would be lost);
//  5. equivalence: S = block text, R = what the editor writes for its node; editable when tidy(S) == tidy(R) or both
//     give the same mdast without positions and marker characters (`_` vs `*`, `*` vs `-` bullets, table pipes).
// An editable block left untouched is saved with its own bytes (compose), so none of this changes a file by itself.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { INLINE_SOURCE, type Core, type LockReason, type MdastNode } from "./core";
import { NO_REFERENCES, referencesOf, type References } from "./references";

export const BOM = "﻿";

export type Verdict = { editable: true; node: PMNode } | { editable: false; reason: LockReason };

/** LF line endings, no line break at the end. */
export function tidy(text: string): string {
  return text.replace(/\r\n|\r/g, "\n").replace(/\n+$/, "");
}

/** References the editor holds no piece for. */
const REFERENCE_TYPES = new Set(["imageReference"]);
const DEFINITION_TYPES = new Set(["definition", "footnoteDefinition"]);
/** Every definition has `]:` right after its label. */
const MAY_DEFINE = "]:";

export function sameMeaning(core: Core, source: string, written: string): boolean {
  if (tidy(source) === tidy(written)) return true;
  try {
    return JSON.stringify(core.mdast(tidy(source))) === JSON.stringify(core.mdast(tidy(written)));
  } catch {
    return false;
  }
}

function holdsDefinition(node: MdastNode): boolean {
  return DEFINITION_TYPES.has(node.type) || (node.children?.some(holdsDefinition) ?? false);
}

/** What the whole text says of references: blocks using one the editor has no piece for, and its definitions. */
export type ReferenceUse = { blocks: Set<number>; references: References };

/**
 * Indexes of blocks (by their start offsets in `text`) that use a picture by reference, and the definitions of
 * `text`. Found with the whole text read once (the reading in which its own references exist). Nothing to find
 * without a `]:`.
 */
export function blocksUsingReferences(
  core: Core,
  text: string,
  starts: readonly number[],
): ReferenceUse {
  const found = new Set<number>();
  if (starts.length === 0 || !text.includes(MAY_DEFINE)) {
    return { blocks: found, references: NO_REFERENCES };
  }
  let tree: MdastNode;
  try {
    tree = core.parseRaw(text);
  } catch {
    // unreadable as a whole: every block with a bracket is held back
    starts.forEach((_, i) => found.add(i));
    return { blocks: found, references: NO_REFERENCES };
  }
  const blockAt = (offset: number): number => {
    let lo = 0;
    let hi = starts.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if ((starts[mid] ?? 0) <= offset) lo = mid;
      else hi = mid - 1;
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
  return { blocks: found, references: referencesOf(tree) };
}

/** Why `doc` (read from a tidied text: LF line endings) is kept: an inline piece spans a line break; null: none does. */
function pieceAcrossLines(doc: PMNode): LockReason | null {
  let found: LockReason | null = null;
  doc.descendants((node) => {
    if (node.type.name === INLINE_SOURCE && String(node.attrs.source).includes("\n")) {
      found = node.attrs.kind === "math" ? "math" : "reference";
    }
    return found === null;
  });
  return found;
}

function definesReference(core: Core, source: string): boolean {
  try {
    return holdsDefinition(core.parseRaw(source));
  } catch {
    return false;
  }
}

/** What one block becomes. `usesReference` from `blocksUsingReferences`; the document's definitions in use. */
export function classify(
  core: Core,
  text: string,
  backendLock: LockReason | null,
  usesReference: boolean,
): Verdict {
  if (backendLock !== null) return { editable: false, reason: backendLock };
  if (text.startsWith(BOM)) return { editable: false, reason: "parse" };
  const source = tidy(text);
  if (source.trim() === "") return { editable: false, reason: "parse" };
  if (source.includes(MAY_DEFINE) && definesReference(core, source)) {
    return { editable: false, reason: "definition" };
  }
  if (usesReference) return { editable: false, reason: "reference" };
  let doc: PMNode;
  let written: string;
  try {
    doc = core.parse(source);
    if (doc.childCount !== 1) return { editable: false, reason: "parse" };
    const across = pieceAcrossLines(doc);
    if (across !== null) return { editable: false, reason: across };
    written = core.serialize(doc);
  } catch {
    return { editable: false, reason: "parse" };
  }
  if (!sameMeaning(core, source, written)) return { editable: false, reason: "differs" };
  return { editable: true, node: doc.child(0) };
}
