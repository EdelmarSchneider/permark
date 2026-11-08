// How each block the visual editor keeps as written looks in View: View's render of the whole text (backend
// `Body::Markdown`, sanitized) laid onto the editor's blocks by line. Only backend HTML comes out of here, cut at element
// boundaries and never made from the source text, so a block can show nothing the backend did not render.
//
// The two are not one to one:
//  - a View block carries the lines of its Markdown node; a raw HTML block has none and takes the last line of the block
//    before it ("floating"): it belongs to the only HTML block between its anchored neighbours;
//  - the front matter is the card's data (`Body.frontMatter`), or a code block when the card is off;
//  - footnote definitions are one block each where written; View lists the referenced ones at the end: by label;
//  - prompt tag lines are markers in View (the `<details>` box around the blocks between them); the box is one View
//    block without lines of its children: a kept block inside gets its own element of the box, matched by order;
//  - link definitions, comments and HTML the sanitizer drops have no View block: nothing to show.
// Unsure -> `source` (the caller keeps the source look): a View block over more than one block (it would show twice),
// floating blocks with several possible owners, a block in a prompt box whose elements cannot be told apart, a
// footnote without its list item, a body that is not Markdown.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import type { Block } from "../../../api/generated/Block";
import type { Body } from "../../../api/generated/Body";
import type { FrontMatter } from "../../../api/generated/FrontMatter";
import type { Composed } from "./compose";
import { LOCKED, reasonOf, type LockReason } from "./lockedView";

/** One top-level block of the editor's text. */
export type LineBlock = {
  /** 1-based. */
  lineStart: number;
  /** Inclusive. */
  lineEnd: number;
  /** `null`: an editable block. */
  reason: LockReason | null;
  text: string;
};

/** What a locked block shows while not being edited. */
export type LockedRender =
  /** View's blocks for its lines, in order. */
  | { kind: "html"; html: string }
  /** View's front matter card. */
  | { kind: "frontMatter"; frontMatter: FrontMatter }
  /** View's list item for this definition, inside View's footnotes section; `first`: the section rule goes above it. */
  | { kind: "footnote"; html: string; first: boolean }
  /** An edge of View's prompt box (View's summary text = `tag`). */
  | { kind: "promptTag"; tag: string; edge: "open" | "close" }
  /** View shows nothing for it. */
  | { kind: "none" }
  | { kind: "source" };

const SOURCE: LockedRender = { kind: "source" };
const NONE: LockedRender = { kind: "none" };

/** Per block of `blocks` (in text order): its View look, `null` for editable blocks. */
export function lockedRenders(body: Body, blocks: readonly LineBlock[]): (LockedRender | null)[] {
  const out = blocks.map((b): LockedRender | null => (b.reason === null ? null : SOURCE));
  if (body.kind !== "markdown") return out;
  const tags = promptTags(body.blocks, blocks);
  const pairs = pairsOf(tags);
  const inBox = new Set<number>();
  for (const pair of pairs) for (let i = pair.open + 1; i < pair.close; i++) inBox.add(i);
  const last = body.blocks.at(-1);
  const section = last !== undefined && isFootnotes(last) ? last : null;
  const views = section === null ? body.blocks : body.blocks.slice(0, -1);
  const owners = ownersOf(views, blocks, tags, inBox);
  const boxed = boxedElements(views, blocks, tags, pairs);
  const mine = new Map<number, string[]>();
  const unsure = new Set<number>();
  views.forEach((view, v) => {
    const own = owners[v] ?? [];
    for (const i of own) {
      if (own.length > 1 || view.html === null) unsure.add(i);
      else mine.set(i, [...(mine.get(i) ?? []), view.html]);
    }
  });
  const items = section === null ? null : footnoteItems(section.html ?? "");
  let firstNote = true;
  blocks.forEach((block, i) => {
    if (block.reason === null) return;
    if (block.reason === "frontMatter" && body.frontMatter !== null) {
      out[i] = { kind: "frontMatter", frontMatter: body.frontMatter };
      return;
    }
    if (block.reason === "footnote") {
      const html = items === null ? null : footnoteOf(items, block.text);
      if (html !== null) {
        out[i] = { kind: "footnote", html, first: firstNote };
        firstNote = false;
      }
      return;
    }
    const tag = tags.get(i);
    if (tag !== undefined) {
      out[i] = { kind: "promptTag", ...tag };
      return;
    }
    if (unsure.has(i)) return;
    // in a box: its own elements of the box, when they can be told apart (a View block of its own too: unsure)
    const html = inBox.has(i) ? (mine.has(i) ? undefined : boxed.get(i)) : (mine.get(i) ?? []);
    if (html !== undefined) {
      out[i] = html.length === 0 ? NONE : { kind: "html", html: html.join("\n") };
    }
  });
  return out;
}

/**
 * View's look of each locked node of `doc`, by node. `composed` places the nodes in `composed.text`, the text `body`
 * renders (an empty paragraph is not written: no lines, no look).
 */
export function lockedLooks(
  body: Body,
  doc: PMNode,
  composed: Composed,
): Map<PMNode, LockedRender> {
  const nodes: PMNode[] = [];
  const blocks: LineBlock[] = [];
  doc.forEach((node, _offset, index) => {
    const range = composed.ranges[index];
    const lineStart = composed.lines.get(node);
    if (!range || lineStart === undefined) return;
    const text = composed.text.slice(range.from, range.to);
    nodes.push(node);
    blocks.push({
      lineStart,
      lineEnd: lineStart + (text.match(/\n/g)?.length ?? 0),
      reason: node.type.name === LOCKED ? reasonOf(node) : null,
      text,
    });
  });
  const looks = new Map<PMNode, LockedRender>();
  lockedRenders(body, blocks).forEach((look, i) => {
    const node = nodes[i];
    if (look !== null && node !== undefined) looks.set(node, look);
  });
  return looks;
}

// ---- lines -> owners -----------------------------------------------------------------------------------------------

/** Index of the first block ending at or after `line` (blocks are in text order and do not overlap). */
function firstEndingFrom(blocks: readonly LineBlock[], line: number): number {
  let lo = 0;
  let hi = blocks.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if ((blocks[mid]?.lineEnd ?? 0) < line) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

function overlapping(blocks: readonly LineBlock[], start: number, end: number): number[] {
  const found: number[] = [];
  for (let i = firstEndingFrom(blocks, start); i < blocks.length; i++) {
    if ((blocks[i]?.lineStart ?? Infinity) > end) break;
    found.push(i);
  }
  return found;
}

/** The blocks each View block stands for. */
function ownersOf(
  views: readonly Block[],
  blocks: readonly LineBlock[],
  tags: ReadonlyMap<number, PromptTag>,
  inBox: ReadonlySet<number>,
): number[][] {
  const owners: number[][] = [];
  // the backend's last line seen: a block without lines of its own gets it (1 before the first block)
  let lastEnd = 0;
  let anchoredEnd = 0;
  let run: number[] = [];
  const settle = (before: number): void => {
    if (run.length === 0) return;
    const candidates: number[] = [];
    for (let i = firstEndingFrom(blocks, anchoredEnd + 1); i < blocks.length; i++) {
      const block = blocks[i];
      if (!block || block.lineStart >= before) break;
      const free = !tags.has(i) && !inBox.has(i);
      if (block.lineStart > anchoredEnd && block.reason === "html" && free) {
        candidates.push(i);
      }
    }
    for (const v of run) owners[v] = candidates;
    run = [];
  };
  views.forEach((view, v) => {
    const single = view.lineStart === view.lineEnd;
    const floating =
      single &&
      (lastEnd === 0
        ? view.lineStart === 1 && unplacedAtStart(blocks)
        : view.lineStart === lastEnd);
    if (isPromptBox(view)) {
      // its blocks get their elements by order (`boxedElements`); its lines anchor the blocks after it
      owners[v] = [];
      if (!floating) {
        settle(view.lineStart);
        anchoredEnd = Math.max(anchoredEnd, view.lineEnd);
      }
    } else if (floating) {
      run.push(v);
      owners[v] = [];
    } else {
      settle(view.lineStart);
      owners[v] = overlapping(blocks, view.lineStart, view.lineEnd);
      anchoredEnd = Math.max(anchoredEnd, view.lineEnd);
    }
    lastEnd = Math.max(lastEnd, view.lineEnd);
  });
  settle(Infinity);
  return owners;
}

/** Line 1 holds no block with lines of its own in View (a View block at line 1 is then a floating one). */
function unplacedAtStart(blocks: readonly LineBlock[]): boolean {
  const first = blocks[0];
  if (!first || first.lineStart > 1) return true;
  return first.reason === "html" || first.reason === "frontMatter" || first.reason === "definition";
}

// ---- HTML of the backend -------------------------------------------------------------------------------------------

/** One tag of the sanitizer's output, where attribute values are double-quoted without `"` and text has no `<`. */
const TAG = /<(\/?)([a-zA-Z][a-zA-Z0-9-]*)((?:[^>"]|"[^"]*")*)>/y;

type Tag = { start: number; end: number; closing: boolean; name: string; attrs: string };

/** The tags of `html` in order; `null` when it is not shaped like the sanitizer's output (then nothing is cut). */
function tagsOf(html: string): Tag[] | null {
  const tags: Tag[] = [];
  let at = html.indexOf("<");
  while (at >= 0) {
    TAG.lastIndex = at;
    const match = TAG.exec(html);
    if (!match) return null;
    tags.push({
      start: at,
      end: TAG.lastIndex,
      closing: match[1] === "/",
      name: (match[2] ?? "").toLowerCase(),
      attrs: match[3] ?? "",
    });
    at = html.indexOf("<", TAG.lastIndex);
  }
  return tags;
}

function openTag(html: string): string {
  return /^<[^>]*>/.exec(html)?.[0] ?? "";
}

function isFootnotes(block: Block): boolean {
  const open = openTag(block.html ?? "");
  return (
    block.kind === "section" && /\sclass="footnotes"/.test(open) && /\sdata-footnotes/.test(open)
  );
}

function isPromptBox(block: Block): boolean {
  return block.kind === "details" && /\sclass="prompt-block"/.test(openTag(block.html ?? ""));
}

// ---- footnotes -----------------------------------------------------------------------------------------------------

type FootnoteItems = { open: string; items: { id: string; html: string }[] };

/** The section's opening tag and its top-level list items (nested lists inside a note stay in their item). */
function footnoteItems(section: string): FootnoteItems | null {
  const tags = tagsOf(section);
  const first = tags?.[0];
  if (!tags || !first || first.name !== "section" || first.closing) return null;
  const items: FootnoteItems["items"] = [];
  let depth = 0;
  let open: Tag | null = null;
  for (const tag of tags) {
    if (tag.name !== "li") continue;
    if (!tag.closing) {
      if (depth === 0) open = tag;
      depth += 1;
      continue;
    }
    depth -= 1;
    if (depth < 0) return null;
    if (depth === 0 && open !== null) {
      const id = /(?:^|\s)id="([^"]*)"/.exec(open.attrs)?.[1] ?? "";
      items.push({ id: idText(id), html: section.slice(open.start, tag.end) });
      open = null;
    }
  }
  return { open: section.slice(0, first.end), items };
}

/** An id attribute as text: entities of the serializer, then the percent escapes of the footnote name. */
function idText(value: string): string {
  const plain = value
    .replace(/&quot;/g, '"')
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&");
  try {
    return decodeURIComponent(plain);
  } catch {
    return plain;
  }
}

/** The label of a footnote definition as the parser names it (trimmed, inner spaces folded to one). */
function footnoteLabel(text: string): string | null {
  const label = /^ {0,3}\[\^([^\]]+)\]:/.exec(text)?.[1];
  return label === undefined ? null : label.trim().replace(/\s+/g, " ");
}

/**
 * View's list item for the definition `text`, alone in a copy of View's section and list (numbered as in View). The
 * wrapper is fixed markup around backend HTML; nothing of it comes from the source.
 */
function footnoteOf(notes: FootnoteItems, text: string): string | null {
  const label = footnoteLabel(text);
  if (label === null) return null;
  const at = notes.items.findIndex((item) => item.id === `fn-${label}`);
  const item = notes.items[at];
  if (item === undefined) return null;
  const list = at === 0 ? "<ol>" : `<ol start="${at + 1}">`;
  return `${notes.open}\n${list}\n${item.html}\n</ol>\n</section>`;
}

// ---- prompt tags ---------------------------------------------------------------------------------------------------

type PromptTag = { tag: string; edge: "open" | "close" };

/** Tag names View drew a prompt box for. */
const BOX_NAME = /<details\b[^>]*\sdata-tag="([A-Za-z0-9_-]+)"/g;
/** `<name>` | `<name attrs>` alone on a line (indent <= 3); `</name>` likewise. */
const OPEN_LINE = /^\s{0,3}<([A-Za-z][A-Za-z0-9_-]*)(?:[ \t].*)?>$/;
const CLOSE_LINE = /^\s{0,3}<\/([A-Za-z][A-Za-z0-9_-]*)\s*>$/;
const SELF_CLOSING = /\/\s*>$/;

/**
 * The paired prompt tag lines, by block index. Pairs as the backend does (a close takes the nearest open of its name;
 * opens above it stay unpaired), over the one-line HTML blocks whose tag has a box in View: a paired tag line is always
 * such a block, and a tag never paired anywhere cannot change how the others pair.
 */
function promptTags(views: readonly Block[], blocks: readonly LineBlock[]): Map<number, PromptTag> {
  const found = new Map<number, PromptTag>();
  const names = new Set<string>();
  for (const view of views) {
    for (const match of (view.html ?? "").matchAll(BOX_NAME)) if (match[1]) names.add(match[1]);
  }
  if (names.size === 0) return found;
  const stack: { at: number; name: string }[] = [];
  blocks.forEach((block, i) => {
    if (block.reason !== "html" || block.lineStart !== block.lineEnd) return;
    const line = block.text.trimEnd();
    const close = CLOSE_LINE.exec(line)?.[1]?.toLowerCase();
    if (close !== undefined) {
      const at = stack.findLastIndex((open) => open.name === close);
      const open = stack[at];
      if (open === undefined) return;
      found.set(open.at, { tag: close, edge: "open" });
      found.set(i, { tag: close, edge: "close" });
      stack.length = at;
      return;
    }
    const name = OPEN_LINE.exec(line)?.[1]?.toLowerCase();
    if (name !== undefined && names.has(name) && !SELF_CLOSING.test(line))
      stack.push({ at: i, name });
  });
  return found;
}

/** A prompt box by the block indexes of its tag lines; `depth`: boxes around it. */
type Pair = { open: number; close: number; depth: number };

/** The boxes of the paired tag lines, in text order (paired lines nest: each close meets its open on top). */
function pairsOf(tags: ReadonlyMap<number, PromptTag>): Pair[] {
  const pairs: Pair[] = [];
  const stack: number[] = [];
  for (const [i, tag] of [...tags].sort(([a], [b]) => a - b)) {
    if (tag.edge === "open") stack.push(i);
    else {
      const open = stack.pop();
      if (open !== undefined) pairs.push({ open, close: i, depth: stack.length });
    }
  }
  return pairs.sort((a, b) => a.open - b.open);
}

/** Elements without a closing tag. */
const VOID = new Set(
  "area base br col embed hr img input link meta param source track wbr".split(" "),
);

/** View's box: its tag and its elements after the summary; null when not shaped so (then nothing in it is matched). */
function boxParts(html: string): { tag: string; children: string[] } | null {
  const tags = tagsOf(html);
  const first = tags?.[0];
  if (!tags || !first || first.name !== "details" || first.closing) return null;
  const tag = /(?:^|\s)data-tag="([^"]*)"/.exec(first.attrs)?.[1];
  if (tag === undefined) return null;
  const children: string[] = [];
  let depth = 0;
  let start = 0;
  let last = first.end;
  for (const [k, t] of tags.entries()) {
    const opens = !t.closing && !VOID.has(t.name) && !t.attrs.trimEnd().endsWith("/");
    if (depth === 1 && !t.closing) {
      // text of its own between the elements: View shows what no block of the editor holds
      if (html.slice(last, t.start).trim() !== "") return null;
      start = t.start;
      if (!opens) {
        children.push(html.slice(t.start, t.end));
        last = t.end;
      }
    }
    if (opens) depth += 1;
    else if (t.closing) {
      depth -= 1;
      if (depth === 1) {
        children.push(html.slice(start, t.end));
        last = t.end;
      } else if (depth === 0) {
        if (k !== tags.length - 1 || html.slice(last, t.start).trim() !== "") return null;
      } else if (depth < 0) return null;
    }
  }
  if (depth !== 0 || !children[0]?.startsWith("<summary")) return null;
  return { tag, children: children.slice(1) };
}

/** Elements a block makes in View's box: one per Markdown block, none for the ones View shows elsewhere | not at all. */
function elementsOf(block: LineBlock): number | null {
  switch (block.reason) {
    case null:
    case "math":
    case "diagram":
    case "data":
    case "kind":
    case "reference":
      return 1;
    case "footnote":
    case "definition":
      return 0;
    // raw HTML makes any number; the others may span several Markdown blocks
    case "html":
    case "parse":
    case "differs":
    case "frontMatter":
      return null;
    default:
      return block.reason satisfies never;
  }
}

/**
 * View's elements for the kept blocks inside prompt boxes, by block index. The editor's outermost boxes meet View's
 * boxes in order; inside one, the blocks take the box's elements in order, as many as each makes: counted from the
 * start up to the first block of unknown count and from the end back to the last one (a single one of unknown count
 * takes what is left). A box inside a box is one element, matched the same way inside it.
 */
function boxedElements(
  views: readonly Block[],
  blocks: readonly LineBlock[],
  tags: ReadonlyMap<number, PromptTag>,
  pairs: readonly Pair[],
): Map<number, string[]> {
  const found = new Map<number, string[]>();
  const outer = pairs.filter((pair) => pair.depth === 0);
  const boxes = views.filter(isPromptBox);
  if (outer.length !== boxes.length) return found;
  const byOpen = new Map(pairs.map((pair) => [pair.open, pair]));

  type Unit = { block: number } | { box: Pair };
  const fill = (html: string, pair: Pair): void => {
    const parts = boxParts(html);
    if (parts === null || parts.tag !== tags.get(pair.open)?.tag) return;
    const units: { unit: Unit; count: number | null }[] = [];
    for (let i = pair.open + 1; i < pair.close; i++) {
      const inner = byOpen.get(i);
      const block = blocks[i];
      if (inner !== undefined) {
        units.push({ unit: { box: inner }, count: 1 });
        i = inner.close;
      } else if (block !== undefined) units.push({ unit: { block: i }, count: elementsOf(block) });
    }
    const all = parts.children;
    const unknown = units.filter((u) => u.count === null);
    const known = units.reduce((sum, u) => sum + (u.count ?? 0), 0);
    const only = unknown[0];
    if (unknown.length === 1 && only !== undefined) only.count = all.length - known;
    if (known > all.length || (unknown.length === 0 && known !== all.length)) return;
    const take = (unit: Unit, elements: string[]): void => {
      if ("box" in unit) {
        const box = elements[0];
        if (box !== undefined) fill(box, unit.box);
      } else if (blocks[unit.block]?.reason !== null) found.set(unit.block, elements);
    };
    let from = 0;
    let rest = units.length;
    for (const [k, u] of units.entries()) {
      if (u.count === null || u.count < 0) {
        rest = k;
        break;
      }
      take(u.unit, all.slice(from, from + u.count));
      from += u.count;
    }
    let to = all.length;
    for (const u of units.slice(rest).reverse()) {
      if (u.count === null || u.count < 0) break;
      take(u.unit, all.slice(to - u.count, to));
      to -= u.count;
    }
  };
  outer.forEach((pair, k) => {
    const html = boxes[k]?.html;
    if (html) fill(html, pair);
  });
  return found;
}
