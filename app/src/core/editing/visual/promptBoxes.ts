// View draws the blocks between a pair of prompt tag lines (`<context>` ... `</context>`) inside one box, its
// `details.prompt-block`. The editor keeps those blocks its own (they stay editable) and draws the same box around
// them: the tag lines are its top and bottom edges (their node views, `lockedView.ts`), node decorations lay the
// blocks between out as the box's children (`markdown.css`), and one frame under the top edge, stretched to the bottom
// edge, paints the box's border and background once (a box of blocks cannot be one element in the editor's DOM).

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { Plugin, PluginKey } from "@milkdown/kit/prose/state";
import { Decoration, DecorationSet, type EditorView } from "@milkdown/kit/prose/view";
import { $prose } from "@milkdown/kit/utils";
import { LOCKED, LOOKS_META, lockedHostCtx, type LockedHost } from "./lockedView";

/** Classes of the blocks inside a box: every one, and the last one (flush with the box's end). */
const IN_BOX = "pm-prompt-in";
const BOX_END = "pm-prompt-end";

/** On a box's open edge: the height from its top to the bottom of the close edge (the frame's height). */
const BOX_HEIGHT = "--prompt-box";

/** A tag line's part in a box. */
type Edge = { tag: string; edge: "open" | "close" };

/** A box by the positions of its tag lines. */
type Box = { open: number; close: number };

type Boxes = {
  /** Tag lines by position, in document order. */
  edges: Map<number, Edge>;
  boxes: Box[];
  decorations: DecorationSet;
};

const promptBoxesKey = new PluginKey<Boxes>("pmPromptBoxes");

/**
 * The tag lines of `doc` by View's looks. A tag line whose look is not known yet (just edited: its look comes with
 * the next render) keeps the part it had at its place before, so the box does not come and go while it is typed in.
 */
function edgesOf(
  doc: PMNode,
  host: LockedHost,
  before: ReadonlyMap<number, Edge>,
): Map<number, Edge> {
  const edges = new Map<number, Edge>();
  doc.forEach((node, pos) => {
    if (node.type.name !== LOCKED) return;
    const look = host.render(node);
    const edge = look === null ? before.get(pos) : look.kind === "promptTag" ? look : undefined;
    if (edge !== undefined) edges.set(pos, { tag: edge.tag, edge: edge.edge });
  });
  return edges;
}

/** The boxes (a close meets the nearest open of its tag), the blocks inside each, and their decorations. */
function boxesOf(doc: PMNode, edges: Map<number, Edge>): Boxes {
  const boxes: Box[] = [];
  const stack: { pos: number; tag: string }[] = [];
  for (const [pos, edge] of edges) {
    if (edge.edge === "open") {
      stack.push({ pos, tag: edge.tag });
      continue;
    }
    const at = stack.findLastIndex((open) => open.tag === edge.tag);
    const open = stack[at];
    if (open === undefined) continue;
    boxes.push({ open: open.pos, close: pos });
    stack.length = at;
  }
  if (boxes.length === 0) return { edges, boxes, decorations: DecorationSet.empty };
  boxes.sort((a, b) => a.open - b.open);

  const nodes: { pos: number; end: number }[] = [];
  doc.forEach((node, pos) => nodes.push({ pos, end: pos + node.nodeSize }));
  const last = new Set<number>();
  for (const box of boxes) {
    const close = nodes.findIndex((n) => n.pos === box.close);
    const before = nodes[close - 1];
    if (before !== undefined && before.pos > box.open) last.add(before.pos);
  }
  // boxes around a node: opens before it minus closes at or before it (boxes nest)
  const closes = boxes.map((b) => b.close).sort((a, b) => a - b);
  const decorations: Decoration[] = [];
  let opened = 0;
  let closed = 0;
  for (const node of nodes) {
    while (opened < boxes.length && (boxes[opened]?.open ?? Infinity) < node.pos) opened += 1;
    while (closed < closes.length && (closes[closed] ?? Infinity) <= node.pos) closed += 1;
    const depth = opened - closed;
    if (depth <= 0) continue;
    const attrs: Record<string, string> = {
      class: last.has(node.pos) ? `${IN_BOX} ${BOX_END}` : IN_BOX,
    };
    if (depth > 1) attrs["style"] = `--prompt-depth: ${depth}`;
    decorations.push(Decoration.node(node.pos, node.end, attrs));
  }
  return { edges, boxes, decorations: DecorationSet.create(doc, decorations) };
}

/**
 * Stretches each box's frame from its open edge to the bottom of its close edge, after every update and whenever the
 * editor's size changes (wrapping, pictures, View's embeds drawn); a box that is gone leaves no frame.
 */
function frames(view: EditorView): { update: () => void; destroy: () => void } {
  let drawn = new Set<HTMLElement>();
  const measure = (): void => {
    const now = new Set<HTMLElement>();
    for (const box of promptBoxesKey.getState(view.state)?.boxes ?? []) {
      const open = view.nodeDOM(box.open);
      const close = view.nodeDOM(box.close);
      if (!(open instanceof HTMLElement) || !(close instanceof HTMLElement)) continue;
      const top = open.getBoundingClientRect().top;
      const height = `${Math.max(0, close.getBoundingClientRect().bottom - top)}px`;
      if (open.style.getPropertyValue(BOX_HEIGHT) !== height) {
        open.style.setProperty(BOX_HEIGHT, height);
      }
      if (open.dataset.box === undefined) open.dataset.box = "";
      now.add(open);
    }
    for (const open of drawn) {
      if (now.has(open)) continue;
      open.style.removeProperty(BOX_HEIGHT);
      delete open.dataset.box;
    }
    drawn = now;
  };
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
  observer?.observe(view.dom);
  measure();
  return { update: measure, destroy: () => observer?.disconnect() };
}

export const promptBoxes = $prose((ctx) => {
  const host = ctx.get(lockedHostCtx.key);
  return new Plugin<Boxes>({
    key: promptBoxesKey,
    state: {
      init: (_config, state) => boxesOf(state.doc, edgesOf(state.doc, host, new Map())),
      apply: (tr, previous) => {
        if (!tr.docChanged && tr.getMeta(LOOKS_META) !== true) return previous;
        const before = new Map<number, Edge>();
        for (const [pos, edge] of previous.edges) before.set(tr.mapping.map(pos, 1), edge);
        return boxesOf(tr.doc, edgesOf(tr.doc, host, before));
      },
    },
    props: { decorations: (state) => promptBoxesKey.getState(state)?.decorations },
    view: frames,
  });
});
