// Marks in Source (session `marks`), one decoration set per layer: a mark decoration per range (an empty range: a
// caret-width widget), the current one with its own class; each layer mapped through every change until its next
// set, and a set of one layer never touches another's. A click on one picks it and leaves the caret where the person
// put it; a right-click opens its layer's menu when it has one. Decoration only: no focus, the insertion widget hidden
// from screen readers. Classes, attributes + routing: `../marks.ts`, look: `../marks.css`.

import {
  RangeSet,
  StateEffect,
  StateField,
  type EditorState,
  type Extension,
  type Range,
} from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
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

type Layer = MarkHandlers & { set: DecorationSet };
/** Every layer, and all their decorations as one set (what the view draws). */
type Marks = { layers: ReadonlyMap<string, Layer>; all: DecorationSet };
/** One layer's new marks (`null`: cleared). */
type LayerSet = { layer: string; marks: Layer | null };

const NONE: Marks = { layers: new Map(), all: Decoration.none };
const setMarks = StateEffect.define<LayerSet>();

/** The caret-width mark of an insertion. */
class InsertMark extends WidgetType {
  constructor(
    readonly layer: string,
    readonly index: number,
    readonly current: boolean,
    readonly base: string | undefined,
  ) {
    super();
  }

  override eq(other: InsertMark): boolean {
    return (
      other.layer === this.layer &&
      other.index === this.index &&
      other.current === this.current &&
      other.base === this.base
    );
  }

  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = markClass(this.current, true, this.base);
    for (const [name, value] of Object.entries(markAttrs(this.layer, this.index))) {
      span.setAttribute(name, value);
    }
    span.setAttribute("aria-hidden", "true");
    return span;
  }

  override ignoreEvent(): boolean {
    return false;
  }
}

function withLayer(value: Marks, { layer, marks }: LayerSet): Marks {
  const layers = new Map(value.layers);
  if (marks === null) layers.delete(layer);
  else layers.set(layer, marks);
  if (layers.size === 0) return NONE;
  return { layers, all: RangeSet.join([...layers.values()].map((l) => l.set)) };
}

const field = StateField.define<Marks>({
  create: () => NONE,
  update: (value, tr) => {
    let next = value;
    if (tr.docChanged && value.layers.size > 0) {
      const layers = new Map<string, Layer>();
      for (const [id, l] of value.layers) layers.set(id, { ...l, set: l.set.map(tr.changes) });
      next = { layers, all: value.all.map(tr.changes) };
    }
    for (const effect of tr.effects) if (effect.is(setMarks)) next = withLayer(next, effect.value);
    return next;
  },
  provide: (f) => EditorView.decorations.from(f, (value) => value.all),
});

/** The marks' state field + click and right-click handlers (part of the Source editor). */
export function markLayers(): Extension {
  return [
    field,
    EditorView.domEventHandlers({
      click: (event, view) => {
        pickClicked(view.state.field(field).layers, event.target);
        // the caret goes where the person clicked, as always
        return false;
      },
      contextmenu: (event, view) => menuClicked(view.state.field(field).layers, event),
    }),
  ];
}

/** The effect putting `ranges` (clamped to the text) as the marks of `layer`; an empty list clears that layer. */
export function marksEffect(
  state: EditorState,
  ranges: readonly TextRange[],
  current: number | null,
  pick: (index: number) => void,
  layer: string = DEFAULT_LAYER,
  options: MarkOptions = {},
): StateEffect<LayerSet> {
  const length = state.doc.length;
  const base = options.className;
  const decorations: Range<Decoration>[] = [];
  ranges.forEach((range, index) => {
    if (range.from < 0 || range.to > length || range.from > range.to) return;
    const now = index === current;
    decorations.push(
      range.from === range.to
        ? Decoration.widget({
            widget: new InsertMark(layer, index, now, base),
            side: 1,
            index,
          }).range(range.from)
        : Decoration.mark({
            class: markClass(now, false, base),
            attributes: markAttrs(layer, index),
            index,
          }).range(range.from, range.to),
    );
  });
  const marks: Layer | null =
    decorations.length === 0
      ? null
      : { set: Decoration.set(decorations, true), pick, menu: options.menu ?? null };
  return setMarks.of({ layer, marks });
}

/** Index of the mark of `layer` holding the caret (also right at its edges); `null`: none. */
export function markAt(state: EditorState, layer: string = DEFAULT_LAYER): number | null {
  const head = state.selection.main.head;
  let found: number | null = null;
  state
    .field(field, false)
    ?.layers.get(layer)
    ?.set.between(head, head, (_from, _to, decoration) => {
      const index: unknown = decoration.spec.index;
      if (typeof index === "number") found = index;
      return false;
    });
  return found;
}
