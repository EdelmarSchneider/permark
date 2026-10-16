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
import { classifyBlock, referenceUses } from "./classify";
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

const REASONS: LockReason[] = [
  "kind",
  "diagram",
  "parse",
  "differs",
  "reference",
  "definition",
  "math",
];

/** Builds the document for a text's blocks. Nothing is edited: composing it gives the text back. */
export function buildDoc(core: Core, input: DocInput): Built {
  const { map } = input;
  const schema = core.view.state.schema;
  const lockedType = schema.nodes[LOCKED];
  const references = referenceUses(core, map);
  const report: LoadReport = {
    editable: 0,
    locked: Object.fromEntries(REASONS.map((r) => [r, 0])) as Record<LockReason, number>,
    reasons: new Map(),
  };
  const originals: PMNode[] = [];
  map.blocks.forEach((block, i) => {
    const verdict = classifyBlock(core, block, references.has(i));
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
          definitions: references.get(i) ?? "",
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

export function isEmptyParagraph(node: PMNode): boolean {
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

/**
 * Reads the document against the blocks it was made from. `saved` is the set of nodes a save could not give a block of the saved
 * text (see `VisualEditor.rebase`): they are what the file holds, so they carry no unsaved-block bar until they are edited.
 */
export function analyze(built: Built, doc: PMNode, saved: WeakSet<PMNode> | null = null): Analysis {
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
    const unsaved =
      skip || saved?.has(node)
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
 * The Markdown the editor writes for one top-level node. A space at the very start or end of a paragraph or heading that the
 * serializer would escape as `&#x20;` is left out (what is left after splitting a sentence in the middle). A plain space at the
 * end is written as it is: it is in the file, and taking it out later is a change.
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

/** How a list starts: bullet or number, and the character that tells it from the lists around it. */
type ListMarker = { ordered: boolean; mark: string };
const BULLET_MARKS = ["-", "*", "+"];
const NUMBER_MARKS = [".", ")"];

/** The marker a list's text starts with, or null when it starts with something else. */
function listMarker(text: string): ListMarker | null {
  const found = /^ {0,3}(?:([-*+])|\d{1,9}([.)]))(?=[ \t\r\n]|$)/.exec(text);
  if (!found) return null;
  return found[1] ? { ordered: false, mark: found[1] } : { ordered: true, mark: found[2] };
}

/** The text of a list the editor wrote, with the marker of its items changed. The editor's items start at the very start of a line and everything inside them is indented. */
function withMarker(text: string, to: ListMarker): string {
  return to.ordered
    ? text.replace(/^(\d{1,9})[.)] /gm, "$1" + to.mark + " ")
    : text.replace(/^[-*+] /gm, to.mark + " ");
}

/** The text of a list as it was written, with the marker of its top-level items changed: the lines inside the items are left as they are. */
function withOriginalMarker(text: string, to: ListMarker): string {
  const lead = /^ {0,3}/.exec(text)?.[0] ?? "";
  return to.ordered
    ? text.replace(new RegExp(`^(${lead})(\\d{1,9})[.)](?=[ \\t])`, "gm"), `$1$2${to.mark}`)
    : text.replace(new RegExp(`^(${lead})[-*+](?=[ \\t])`, "gm"), `$1${to.mark}`);
}

/**
 * Two lists of the same kind side by side are one list in Markdown, unless their markers differ. The editor writes the lists it
 * rewrites with its own marker (`-` and `1.`), so a list that was kept apart from its neighbour by its marker alone, or
 * one made from a paragraph next to a list, would join the list beside it. A list the editor writes takes another marker
 * when its neighbours use the one it would have. The lists that are kept as they were keep their own, except one that now sits
 * right after a list of its marker (what was between them is gone): its items take another marker (and nothing else of it
 * changes, `original` gives its text as it was), so that the two stay two lists in the file, as they are in the view, and the
 * file and the view agree after the save.
 */
function keepListsApart(
  slots: Slot[],
  markers: (ListMarker | null)[],
  rewritten: boolean[],
  original: (slot: number) => string | null,
): void {
  for (let i = 0; i < slots.length; i++) {
    const here = markers[i];
    if (!here) continue;
    const before = markers[i - 1];
    // a list kept as it was gives way only to the list before it (the one after it gives way to this one in its turn)
    if (
      !rewritten[i] &&
      !(before && before.ordered === here.ordered && before.mark === here.mark)
    ) {
      continue;
    }
    const taken = new Set<string>();
    for (const k of [i - 1, i + 1]) {
      const other = markers[k];
      if (!other || other.ordered !== here.ordered) continue;
      // a rewritten list further down still has to choose: it will take what is left
      if (k < i || !rewritten[k]) taken.add(other.mark);
    }
    if (!taken.has(here.mark)) continue;
    const mark = (here.ordered ? NUMBER_MARKS : BULLET_MARKS).find((m) => !taken.has(m));
    if (mark === undefined) continue;
    const to = { ordered: here.ordered, mark };
    if (rewritten[i]) {
      slots[i].text = withMarker(slots[i].text as string, to);
    } else {
      const text = original(i);
      if (text === null) continue;
      slots[i] = { origin: slots[i].origin, text: withOriginalMarker(text, to), retyped: false };
      rewritten[i] = true;
    }
    markers[i] = { ordered: here.ordered, mark };
  }
}

const isListNode = (node: PMNode): boolean =>
  node.type.name === "bullet_list" || node.type.name === "ordered_list";

/** The opening fence of a code block as written (its indent, its fence and the spacing before the info string), and the info string's first word. */
const OPENING_FENCE = /^( {0,3}(?:`{3,}|~{3,})[ \t]*)(\S*)/;

/**
 * A fenced code block whose only difference from the block it was made from is its language is saved as that block's own text with the
 * info string of its opening fence replaced: the fence characters (`~~~`, four backticks), the indent, the code and the closing
 * fence stay as they were written, so a change of language changes one line of the file. Null for any other block.
 */
function withFenceLanguage(built: Built, origin: number | null, node: PMNode): string | null {
  if (origin === null || node.type.name !== "code_block") return null;
  const original = built.originals[origin];
  const block = built.map.blocks[origin];
  if (!original || original.type !== node.type || block?.kind !== "code") return null;
  if (!node.content.eq(original.content)) return null;
  const language = String(node.attrs.language ?? "");
  if (language === String(original.attrs.language ?? "")) return null;
  if (
    !Object.keys(node.attrs).every((k) => k === "language" || node.attrs[k] === original.attrs[k])
  )
    return null;
  const fence = OPENING_FENCE.exec(block.text);
  if (!fence) return null;
  // no info string left: no spacing either
  const opening = language === "" ? fence[1].replace(/[ \t]+$/, "") : fence[1];
  return opening + language + block.text.slice(fence[0].length);
}

/** The blocks to save, in order: untouched ones by their index, the others with the text the editor writes for them. */
export function slotsOf(core: Core, built: Built, analysis: Analysis): Slot[] {
  const slots: Slot[] = [];
  const markers: (ListMarker | null)[] = [];
  const rewritten: boolean[] = [];
  const push = (slot: Slot, marker: ListMarker | null, written: boolean): void => {
    slots.push(slot);
    markers.push(marker);
    rewritten.push(written);
  };
  for (const info of analysis.nodes) {
    if (info.skip) continue;
    const { origin } = info;
    if (info.untouched || (info.node.type.name === LOCKED && origin !== null)) {
      const block = origin === null ? undefined : built.map.blocks[origin];
      // Indented by two spaces or more, a block right after a list the editor writes would read as part of its last item. It was not
      // next to a list when it was written, so it can only happen once the block before it became one: it is written without the indent.
      const last = slots.length - 1;
      if (
        info.untouched &&
        block !== undefined &&
        info.node.type.name !== LOCKED &&
        last >= 0 &&
        rewritten[last] &&
        markers[last] !== null &&
        /^ {2,}\S/.test(block.text)
      ) {
        push({ origin, text: writeNode(core, info.node) }, null, true);
        continue;
      }
      push({ origin }, block?.kind === "list" ? listMarker(block.text) : null, false);
    } else if (info.node.type.name === LOCKED) {
      push({ origin: null, text: info.node.attrs.source as string }, null, false);
    } else {
      const text = withFenceLanguage(built, origin, info.node) ?? writeNode(core, info.node);
      const retyped = origin !== null && info.node.type !== built.originals[origin]?.type;
      push({ origin, text, retyped }, isListNode(info.node) ? listMarker(text) : null, true);
    }
  }
  keepListsApart(slots, markers, rewritten, (i) => {
    const origin = slots[i].origin;
    return origin === null ? null : (built.map.blocks[origin]?.text ?? null);
  });
  return slots;
}

/** The text to save for the document as it is now (the file's BOM, line endings and every untouched byte). */
export function composeDoc(core: Core, built: Built, doc: PMNode): Composed {
  return composeDetailed(built.map, slotsOf(core, built, analyze(built, doc)));
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
