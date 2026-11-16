// Marks in Edit mode (session `marks`), one decoration set per layer: text offsets -> document positions
// (`docRange`), an inline decoration per range found (an empty range: a caret-width widget), the current one with its
// own class; set by a meta transaction kept out of the undo history, each layer mapped through every change until its
// next set, and a set of one layer never touches another's. A range not found in its block (markup inside, a locked
// block) gets no mark. A click on one picks it and leaves the caret alone; a right-click opens its layer's menu when it
// has one. Classes, attributes + routing: `../marks.ts`, look: `../marks.css`.

import { Plugin, PluginKey, type EditorState } from "@milkdown/kit/prose/state";
import { Decoration, DecorationSet, type EditorView } from "@milkdown/kit/prose/view";
import {
  DEFAULT_LAYER,
  markAttrs,
  markClass,
  menuClicked,
  pickClicked,
  type MarkHandlers,
  type MarkOptions,
} from "../marks";
import type { TextRange } from "../session.svelte";
import type { Composed } from "./compose";
import { docRange } from "./edits";

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
        const all = [...layers.values()].flatMap((l) => l.set.find());
        return { layers, all: DecorationSet.create(state.doc, all) };
      },
    },
    props: {
      decorations: (state) => marksKey.getState(state)?.all ?? null,
      handleDOMEvents: {
        click: (view, event) => {
          pickClicked(marksKey.getState(view.state)?.layers ?? NONE.layers, event.target);
          // the caret goes where the person clicked, as always
          return false;
        },
        contextmenu: (view, event) =>
          menuClicked(marksKey.getState(view.state)?.layers ?? NONE.layers, event),
      },
    },
  });
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
  ranges.forEach((range, index) => {
    const found = docRange(doc, composed, range);
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
