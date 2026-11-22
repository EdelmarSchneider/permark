// Marks in Edit mode (session `marks`), one decoration set per layer: text offsets -> document positions
// (`docRanges`), an inline decoration per range found (an empty range: a caret-width widget), the current one with its
// own class; set by a meta transaction kept out of the undo history, each layer mapped through every change until its
// next set, and a set of one layer never touches another's. A range not found in its block (markup inside, a locked
// block) gets no mark. A click on one picks it and leaves the caret alone; a right-click opens its layer's menu when it
// has one. Classes, attributes + routing: `../marks.ts`, look: `../marks.css`.

import { Plugin, PluginKey, TextSelection, type EditorState } from "@milkdown/kit/prose/state";
import { Decoration, DecorationSet, type EditorView } from "@milkdown/kit/prose/view";
import {
  DEFAULT_LAYER,
  markAttrs,
  markClass,
  menuClicked,
  onMark,
  pickClicked,
  type MarkHandlers,
  type MarkOptions,
} from "../marks";
import type { TextRange } from "../session.svelte";
import type { Composed } from "./compose";
import { docRanges } from "./edits";

type Layer = MarkHandlers & { set: DecorationSet };
/** Every layer, and all their decorations as one set (what the view draws). */
type Marks = { layers: ReadonlyMap<string, Layer>; all: DecorationSet };
/** One layer's new marks (`null`: cleared). */
type LayerSet = { layer: string; marks: Layer | null };

const NONE: Marks = { layers: new Map(), all: DecorationSet.empty };
const marksKey = new PluginKey<Marks>("PM_CORRECTION_MARKS");

function isLayerSet(value: unknown): value is LayerSet {
  return typeof value === "object" && value !== null && "layer" in value && "marks" in value;
}

export function marksPlugin(): Plugin<Marks> {
  return new Plugin<Marks>({
    key: marksKey,
    state: {
      init: () => NONE,
      apply: (tr, value, _old, state) => {
        let next = value;
        if (tr.docChanged && value.layers.size > 0) {
          const layers = new Map<string, Layer>();
          for (const [id, l] of value.layers) {
            layers.set(id, { ...l, set: l.set.map(tr.mapping, tr.doc) });
          }
          next = { layers, all: value.all.map(tr.mapping, tr.doc) };
        }
        const meta: unknown = tr.getMeta(marksKey);
        if (!isLayerSet(meta)) return next;
        const layers = new Map(next.layers);
        if (meta.marks === null) layers.delete(meta.layer);
        else layers.set(meta.layer, meta.marks);
        if (layers.size === 0) return NONE;
        const sets = [...layers.values()];
        // one layer: its own set is the one drawn (a set made again from its marks is the same, at the same cost as
        // the first: every top-level block against every mark)
        if (sets.length === 1 && sets[0]) return { layers, all: sets[0].set };
        const all = sets.flatMap((l) => l.set.find());
        return { layers, all: DecorationSet.create(state.doc, all) };
      },
    },
    props: {
      decorations: (state) => marksKey.getState(state)?.all ?? null,
      handleDOMEvents: {
        click: (view, event) => {
          // the caret goes where the person clicked, as always: a click quicker than the browser's news of the caret
          // it placed (a short press, automation) puts it into the state first, else the marks drawn anew for the
          // pick would write the earlier selection (a suggestion shown before) back over the click
          if (onMark(event.target)) takePageSelection(view);
          pickClicked(marksKey.getState(view.state)?.layers ?? NONE.layers, event.target);
          return false;
        },
        contextmenu: (view, event) =>
          menuClicked(marksKey.getState(view.state)?.layers ?? NONE.layers, event),
      },
    },
  });
}

/** The page's selection into the editor's state when they differ: the browser placed it, the editor has not read it
 *  yet. One that leaves the editor | names no place of its document: nothing. */
function takePageSelection(view: EditorView): void {
  const page = view.dom.ownerDocument.getSelection();
  const anchorNode = page?.anchorNode;
  const focusNode = page?.focusNode;
  if (!page || !anchorNode || !focusNode) return;
  if (!view.dom.contains(anchorNode) || !view.dom.contains(focusNode)) return;
  let anchor: number;
  let head: number;
  try {
    anchor = view.posAtDOM(anchorNode, page.anchorOffset);
    head = view.posAtDOM(focusNode, page.focusOffset);
  } catch {
    return;
  }
  const { doc, selection } = view.state;
  if (selection.anchor === anchor && selection.head === head) return;
  const placed = TextSelection.between(doc.resolve(anchor), doc.resolve(head));
  view.dispatch(view.state.tr.setSelection(placed).setMeta("pointer", true));
}

function insertMark(layer: string, index: number, current: boolean, base?: string): HTMLElement {
  const span = document.createElement("span");
  span.className = markClass(current, true, base);
  for (const [name, value] of Object.entries(markAttrs(layer, index))) {
    span.setAttribute(name, value);
  }
  span.setAttribute("aria-hidden", "true");
  return span;
}

/** Marks `ranges` (offsets of `composed.text`) on `layer`; an empty list clears that layer. */
export function setMarks(
  view: EditorView,
  composed: Composed,
  ranges: readonly TextRange[],
  current: number | null,
  pick: (index: number) => void,
  layer: string = DEFAULT_LAYER,
  options: MarkOptions = {},
): void {
  const doc = view.state.doc;
  const base = options.className;
  const decorations: Decoration[] = [];
  docRanges(doc, composed, ranges).forEach((found, index) => {
    if (found === null) return;
    const now = index === current;
    decorations.push(
      found.from === found.to
        ? Decoration.widget(found.from, () => insertMark(layer, index, now, base), {
            side: 1,
            index,
            key: `${layer}-${index}-${now}`,
            ignoreSelection: true,
          })
        : Decoration.inline(
            found.from,
            found.to,
            { class: markClass(now, false, base), ...markAttrs(layer, index) },
            { index },
          ),
    );
  });
  const marks: Layer | null =
    decorations.length === 0
      ? null
      : { set: DecorationSet.create(doc, decorations), pick, menu: options.menu ?? null };
  const meta: LayerSet = { layer, marks };
  view.dispatch(view.state.tr.setMeta(marksKey, meta).setMeta("addToHistory", false));
}

/** Index of the mark of `layer` holding the caret (also right at its edges); `null`: none. */
export function markAt(state: EditorState, layer: string = DEFAULT_LAYER): number | null {
  const head = state.selection.head;
  const found = marksKey.getState(state)?.layers.get(layer)?.set.find(head, head) ?? [];
  const index: unknown = found[0]?.spec.index;
  return typeof index === "number" ? index : null;
}

/** Index of the first of `ranges` (offsets of `composed.text`, sorted, not overlapping) holding the caret, also right
 *  at its edges, among those `setMarks` would mark: what `markAt` answers once they are marked; `null`: none. Only
 *  the ranges inside the caret's top-level block are placed (a mark never leaves its block). */
export function rangeAt(
  state: EditorState,
  composed: Composed,
  ranges: readonly TextRange[],
): number | null {
  const head = state.selection.head;
  const block = composed.ranges[state.selection.$head.index(0)];
  if (!block) return null;
  const near: number[] = [];
  const inBlock: TextRange[] = [];
  ranges.forEach((range, index) => {
    if (block.from > range.from || range.to > block.to) return;
    near.push(index);
    inBlock.push(range);
  });
  const found = docRanges(state.doc, composed, inBlock);
  const at = found.findIndex((place) => place !== null && place.from <= head && head <= place.to);
  return at < 0 ? null : (near[at] ?? null);
}
