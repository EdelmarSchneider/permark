// Which blocks Edit mode may change. A block becomes an editable node only when the editor writes it back as the
// same Markdown; else it is a locked atom keeping its bytes. In order:
//  1. backend kind (front matter, HTML, math, footnotes, diagrams, definitions, alerts, ...) - `segment.lock`;
//  2. content: inline math `$x$` (the editor reads `$a_b_c$` as emphasis), a BOM at the start (parser drops it);
//  3. references: a block holding a link | footnote definition (the editor resolves and drops it), or using one
//     defined elsewhere (alone, it reads as text and is written escaped);
//  4. the editor's reading: exactly one top-level node;
//  5. equivalence: S = block text, R = what the editor writes for its node; editable when tidy(S) == tidy(R) or both
//     give the same mdast without positions and marker characters (`_` vs `*`, `*` vs `-` bullets, table pipes).
// An editable block left untouched is saved with its own bytes (compose), so none of this changes a file by itself.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import type { Core, LockReason, MdastNode } from "./core";

export const BOM = "﻿";

export type Verdict = { editable: true; node: PMNode } | { editable: false; reason: LockReason };

/** LF line endings, no line break at the end. */
export function tidy(text: string): string {
  return text.replace(/\r\n|\r/g, "\n").replace(/\n+$/, "");
}

/** Something that looks like inline math: `$x$`, `$a_b$`. */
export function hasInlineMath(text: string): boolean {
  return /\$[^\s$](?:[^$\n]*[^\s$\\])?\$/.test(text);
}

const REFERENCE_TYPES = new Set(["linkReference", "imageReference", "footnoteReference"]);
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

/**
 * Indexes of blocks (by their start offsets in `text`) that use a reference defined elsewhere. Found with the whole
 * text read once (the only reading in which references exist). Nothing to find without a `]:`.
 */
export function blocksUsingReferences(
  core: Core,
  text: string,
  starts: readonly number[],
): Set<number> {
  const found = new Set<number>();
  if (starts.length === 0 || !text.includes(MAY_DEFINE)) return found;
  let tree: MdastNode;
  try {
    tree = core.parseRaw(text);
  } catch {
    // unreadable as a whole: every block with a bracket is held back
    starts.forEach((_, i) => found.add(i));
    return found;
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
  return found;
}

function definesReference(core: Core, source: string): boolean {
  try {
    return holdsDefinition(core.parseRaw(source));
  } catch {
    return false;
  }
}

/** What one block becomes. `usesReference` from `blocksUsingReferences`. */
export function classify(
  core: Core,
  text: string,
  backendLock: LockReason | null,
  usesReference: boolean,
): Verdict {
  if (backendLock !== null) return { editable: false, reason: backendLock };
  if (hasInlineMath(text)) return { editable: false, reason: "math" };
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
    written = core.serialize(doc);
  } catch {
    return { editable: false, reason: "parse" };
  }
  if (!sameMeaning(core, source, written)) return { editable: false, reason: "differs" };
  return { editable: true, node: doc.child(0) };
}
