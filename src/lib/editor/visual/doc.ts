// The Visual document and what saving makes of it (docs/design/v0.2.md section 3).
//
// `buildDoc` turns the blocks of a text into a ProseMirror document, one top-level node per block: an editable node
// made from the block's own text, or a locked atom. Every top-level node carries `tmBlock`, the index of its block.
//
// `analyze` reads the document as it is now. A top-level node is untouched while it is equal to the node made at load, so
// splits, joins, pastes and undo need no tracking of their own: put the text back and the block is clean again. An untouched
// block is saved with its original bytes; any other one is written by the editor. The result is a list of `Slot`s for
// `compose()`, and the set of nodes that show the unsaved-block bar.

import { Fragment, type Node as PMNode } from "@milkdown/kit/prose/model";
import type { BlockKind } from "../../types";
import {
  composeDetailed,
  countLineBreaks,
  type BlockMap,
  type Composed,
  type Slot,
} from "../blockModel";
import { blocksWithReferences, classifyBlock } from "./classify";
import { LOCKED, TM_BLOCK, type Core, type LockReason } from "./core";

/** What goes into a Visual document. */
export type DocInput = {
  /** The blocks of the text being edited. */
  map: BlockMap;
  /** For every block of `map`: it differs from the saved file (it came from unsaved Source edits). */
  unsaved?: readonly boolean[];
};

/** How the blocks of a loaded document turned out. */
export type LoadReport = {
  editable: number;
  locked: Record<LockReason, number>;
  /** Block index to why it is locked, for the ones that are. */
  reasons: Map<number, LockReason>;
};

/** A document and what it was made from. */
export type Built = {
  doc: PMNode;
  /** The node made for every block of the input, as it was when loaded. */
  originals: PMNode[];
  unsaved: boolean[];
  map: BlockMap;
  report: LoadReport;
};

const REASONS: LockReason[] = ["kind", "diagram", "parse", "differs", "reference", "math"];

/** Builds the document for a text's blocks. Nothing is edited: composing it gives the text back. */
export function buildDoc(core: Core, input: DocInput): Built {
  const { map } = input;
  const schema = core.view.state.schema;
  const lockedType = schema.nodes[LOCKED];
  const withReferences = blocksWithReferences(core, map);
  const report: LoadReport = {
    editable: 0,
    locked: Object.fromEntries(REASONS.map((r) => [r, 0])) as Record<LockReason, number>,
    reasons: new Map(),
  };
  const originals: PMNode[] = [];
  map.blocks.forEach((block, i) => {
    const verdict = classifyBlock(core, block, withReferences.has(i));
    if (verdict.editable) {
      const node = verdict.node;
      report.editable++;
      originals.push(node.type.create({ ...node.attrs, [TM_BLOCK]: i }, node.content, node.marks));
    } else {
      report.locked[verdict.reason]++;
      report.reasons.set(i, verdict.reason);
      originals.push(
        lockedType.create({
          source: block.text,
          kind: block.kind satisfies BlockKind,
          reason: verdict.reason,
          [TM_BLOCK]: i,
        }),
      );
    }
  });
  const children = [...originals];
  // something to type in: a document with no block, or one that ends in an atom, gets an empty paragraph (never written)
  if (children.length === 0 || children[children.length - 1].type === lockedType) {
    children.push(schema.nodes.paragraph.create());
  }
  const doc = schema.topNodeType.create(null, Fragment.fromArray(children));
  const unsaved = map.blocks.map((_, i) => input.unsaved?.[i] ?? false);
  return { doc, originals, unsaved, map, report };
}

/** The index of the original block a top-level node stands for, or null for a new one. */
export function originOf(node: PMNode): number | null {
  const value = node.attrs[TM_BLOCK] as unknown;
  return typeof value === "number" ? value : null;
}

function isEmptyParagraph(node: PMNode): boolean {
  return node.type.name === "paragraph" && node.content.size === 0;
}

/** One top-level node of the document, read against the block it was made from. */
export type NodeInfo = {
  node: PMNode;
  /** An empty paragraph: it holds no text and is not written. */
  skip: boolean;
  /** The original block this node is, or null for a new one. */
  origin: number | null;
  /** The node is a block of the text exactly as loaded (it is saved with the block's own bytes). */
  untouched: boolean;
  /** It shows the unsaved-block bar. */
  unsaved: boolean;
};

/** The document read against the blocks it was made from. Cheap: nothing is written until `slotsOf()`. */
export type Analysis = {
  nodes: NodeInfo[];
  /** The set of blocks differs from the text (one was deleted, added or moved). */
  structure: boolean;
  /** How many nodes carry the unsaved-block bar. */
  changed: number;
  /** How many written nodes are not exactly their original block (edited or new). */
  touched: number;
};

/** Reads the document against the blocks it was made from. */
export function analyze(built: Built, doc: PMNode): Analysis {
  const { originals, unsaved: wasUnsaved, map } = built;
  const children: PMNode[] = [];
  doc.forEach((child) => children.push(child));

  // A block can be claimed by several nodes (a split copies it): the one still equal to the original owns it, else the first with text.
  const owner = new Map<number, number>();
  children.forEach((child, i) => {
    const origin = originOf(child);
    if (origin === null || origin >= originals.length) return;
    const current = owner.get(origin);
    if (current === undefined) {
      owner.set(origin, i);
      return;
    }
    const original = originals[origin];
    const better = children[current].eq(original)
      ? false
      : child.eq(original) || (isEmptyParagraph(children[current]) && !isEmptyParagraph(child));
    if (better) owner.set(origin, i);
  });

  const nodes: NodeInfo[] = children.map((node, i) => {
    const claimed = originOf(node);
    const origin = claimed !== null && owner.get(claimed) === i ? claimed : null;
    const skip = isEmptyParagraph(node);
    const untouched = !skip && origin !== null && node.eq(originals[origin]);
    const fromSource = origin !== null && wasUnsaved[origin];
    // an atom is never rewritten, so only the Source edits it came with (or being a copy) make it unsaved
    const unsaved = skip
      ? false
      : node.type.name === LOCKED
        ? origin === null || fromSource
        : !untouched || fromSource;
    return { node, skip, origin, untouched, unsaved };
  });
  const written = nodes.filter((n) => !n.skip);
  const structure = written.length !== map.blocks.length || written.some((n, i) => n.origin !== i);
  return {
    nodes,
    structure,
    changed: nodes.filter((n) => n.unsaved).length,
    touched: written.filter((n) => !n.untouched).length,
  };
}

const written = new WeakMap<PMNode, string>();

/**
 * The Markdown the editor writes for one top-level node. A space at the very start or end of a paragraph or heading (what is left
 * after splitting a sentence in the middle) is not written as `&#x20;`: Markdown drops it anyway.
 */
export function writeNode(core: Core, node: PMNode): string {
  let text = written.get(node);
  if (text === undefined) {
    text = core.serialize(node.type.schema.topNodeType.create(null, node));
    if (node.type.name === "paragraph" || node.type.name === "heading") {
      text = text.replace(/^(?:&#x20;)+/, "").replace(/(?:&#x20;)+(?=\n*$)/, "");
    }
    written.set(node, text);
  }
  return text;
}

/** The blocks to save, in order: untouched ones by their index, the others with the text the editor writes for them. */
export function slotsOf(core: Core, analysis: Analysis): Slot[] {
  const slots: Slot[] = [];
  for (const info of analysis.nodes) {
    if (info.skip) continue;
    if (info.untouched || (info.node.type.name === LOCKED && info.origin !== null)) {
      slots.push({ origin: info.origin });
    } else if (info.node.type.name === LOCKED) {
      slots.push({ origin: null, text: info.node.attrs.source as string });
    } else slots.push({ origin: info.origin, text: writeNode(core, info.node) });
  }
  return slots;
}

/** The text to save for the document as it is now (the file's BOM, line endings and every untouched byte). */
export function composeDoc(core: Core, built: Built, doc: PMNode): Composed {
  return composeDetailed(built.map, slotsOf(core, analyze(built, doc)));
}

/** The slot of every top-level node (-1 for one that is not written), for placing a caret in the composed text. */
export function slotIndexes(analysis: Analysis): number[] {
  let next = 0;
  return analysis.nodes.map((n) => (n.skip ? -1 : next++));
}

/** The 0-based line, in a composed text, where a top-level node's block starts. */
export function lineOfNode(composed: Composed, analysis: Analysis, child: number): number {
  const slotOf = slotIndexes(analysis);
  let slot = slotOf[child] ?? -1;
  // an empty paragraph is not written: the caret goes to the block before it (or the one after, at the top)
  for (let i = child - 1; slot < 0 && i >= 0; i--) slot = slotOf[i];
  for (let i = child + 1; slot < 0 && i < slotOf.length; i++) slot = slotOf[i];
  if (slot < 0) return 0;
  return countLineBreaks(composed.text.slice(0, composed.starts[slot]));
}
