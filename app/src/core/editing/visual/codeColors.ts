// Token colors of code blocks in Edit mode: the runs the host's colorer gives for a block's text (View's highlighter,
// so View's classes and theme tokens), drawn as inline decorations, one element per class around a run, nested as
// View nests them (the inner token's color wins, as there). The document's text and attributes are never touched.
//
// A changed block is colored again once typing has rested for a moment, never while a composition is open; until
// then its previous colors are mapped through the edits. Each colored block carries a node decoration naming the node
// its colors were made for: a block that is new, changed, or put back by an undo is a different node, so it is asked
// again, and only it.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { Plugin, PluginKey } from "@milkdown/kit/prose/state";
import { Decoration, DecorationSet } from "@milkdown/kit/prose/view";
import type { CodeSpan } from "../../plugins/props";
import type { CodeHost } from "./core";

/** How long typing rests before the changed blocks are colored again. */
export const COLORS_DELAY_MS = 150;

type Colors = {
  set: DecorationSet;
  /** No transaction since the document was set: its blocks are colored at once. */
  fresh: boolean;
};

/** The colors of one block, for the node at `at` they were made for. */
type Painted = { at: number; node: PMNode; spans: readonly CodeSpan[] };

type Block = { node: PMNode; at: number };

const colorsKey = new PluginKey<Colors>("PM_CODE_COLORS");

function isPainted(value: unknown): value is Painted {
  return (
    typeof value === "object" &&
    value !== null &&
    "at" in value &&
    "node" in value &&
    "spans" in value
  );
}

/** The code blocks of a document (text and locked blocks are not looked into). */
function codeBlocks(doc: PMNode): Block[] {
  const found: Block[] = [];
  doc.descendants((node, at) => {
    if (node.type.name === "code_block") {
      found.push({ node, at });
      return false;
    }
    return !node.isTextblock && !node.isAtom;
  });
  return found;
}

function colored(set: DecorationSet, { node, at }: Block): boolean {
  const mine = (spec: { colorsOf?: unknown }): boolean => spec.colorsOf === node;
  return set.find(at, at + node.nodeSize, mine).length > 0;
}

/** `set` with the block's old colors replaced by `spans`. */
function paint(set: DecorationSet, doc: PMNode, { at, node, spans }: Painted): DecorationSet {
  const end = at + node.nodeSize;
  const old = set.find(at, end).filter((d) => d.from >= at && d.to <= end);
  const start = at + 1;
  const size = node.content.size;
  const drawn = [Decoration.node(at, end, {}, { colorsOf: node })];
  for (const { from, to, classes } of spans) {
    if (from < 0 || from >= to || to > size) continue;
    // decorations over the same text are drawn first-listed innermost: the innermost class goes first
    for (const className of classes.toReversed()) {
      drawn.push(
        Decoration.inline(start + from, start + to, { nodeName: "span", class: className }),
      );
    }
  }
  return set.remove(old).add(doc, drawn);
}

/** Colors the code blocks with what `host` offers for their language (read when a block is colored). */
export function codeColors(host: () => CodeHost): Plugin<Colors> {
  return new Plugin<Colors>({
    key: colorsKey,
    state: {
      init: () => ({ set: DecorationSet.empty, fresh: true }),
      apply: (tr, value) => {
        const set = tr.docChanged ? value.set.map(tr.mapping, tr.doc) : value.set;
        const meta: unknown = tr.getMeta(colorsKey);
        // colors for a node that is no longer there come too late: the next round asks again
        if (!isPainted(meta) || tr.doc.nodeAt(meta.at) !== meta.node) return { set, fresh: false };
        return { set: paint(set, tr.doc, meta), fresh: false };
      },
    },
    props: {
      decorations: (state) => colorsKey.getState(state)?.set ?? null,
    },
    view: (view) => {
      let timer: ReturnType<typeof setTimeout> | undefined;
      let gone = false;
      const asking = new WeakSet<PMNode>();
      const schedule = (delay: number): void => {
        clearTimeout(timer);
        timer = setTimeout(round, delay);
      };
      const ask = async (node: PMNode): Promise<void> => {
        asking.add(node);
        const language = String(node.attrs.language ?? "");
        let spans: readonly CodeSpan[] = [];
        try {
          const colorer = await host().colors?.(language);
          spans = (await colorer?.(node.textContent, language)) ?? [];
        } catch {
          // the highlighter did not load or failed on this text: the block stays plain
        } finally {
          asking.delete(node);
        }
        if (gone) return;
        // redrawing the text under an open composition would break it: asked again once it ends
        if (view.composing) {
          schedule(COLORS_DELAY_MS);
          return;
        }
        const at = codeBlocks(view.state.doc).find((block) => block.node === node)?.at;
        if (at === undefined) return;
        const painted: Painted = { at, node, spans };
        view.dispatch(view.state.tr.setMeta(colorsKey, painted).setMeta("addToHistory", false));
      };
      function round(): void {
        timer = undefined;
        if (view.composing) {
          schedule(COLORS_DELAY_MS);
          return;
        }
        const set = colorsKey.getState(view.state)?.set ?? DecorationSet.empty;
        for (const block of codeBlocks(view.state.doc)) {
          if (!asking.has(block.node) && !colored(set, block)) void ask(block.node);
        }
      }
      schedule(0);
      return {
        update: (next, previous) => {
          if (next.state.doc === previous.doc) return;
          schedule(colorsKey.getState(next.state)?.fresh === true ? 0 : COLORS_DELAY_MS);
        },
        destroy: () => {
          gone = true;
          clearTimeout(timer);
        },
      };
    },
  });
}
