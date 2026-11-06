// Correction marks in Source (session `marks`): a mark decoration per range (an empty range: a caret-width widget),
// the current one with its own class; mapped through every change until the next set. A click on one picks it and
// leaves the caret where the person put it. Decoration only: no focus, the insertion widget hidden from screen readers.
// Classes + look: `../marks.ts`, `../marks.css`.

import {
  StateEffect,
  StateField,
  type EditorState,
  type Extension,
  type Range,
} from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
import { clickedMark, markClass } from "../marks";
import type { TextRange } from "../session.svelte";

type Marks = { set: DecorationSet; pick: ((index: number) => void) | null };

const NONE: Marks = { set: Decoration.none, pick: null };
const setMarks = StateEffect.define<Marks>();

/** The caret-width mark of an insertion. */
class InsertMark extends WidgetType {
  constructor(
    readonly index: number,
    readonly current: boolean,
  ) {
    super();
  }

  override eq(other: InsertMark): boolean {
    return other.index === this.index && other.current === this.current;
  }

  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = markClass(this.current, true);
    span.setAttribute("data-correction-mark", String(this.index));
    span.setAttribute("aria-hidden", "true");
    return span;
  }

  override ignoreEvent(): boolean {
    return false;
  }
}

const field = StateField.define<Marks>({
  create: () => NONE,
  update: (value, tr) => {
    for (const effect of tr.effects) if (effect.is(setMarks)) return effect.value;
    return tr.docChanged ? { ...value, set: value.set.map(tr.changes) } : value;
  },
  provide: (f) => EditorView.decorations.from(f, (value) => value.set),
});

/** The marks' state field + click handler (part of the Source editor). */
export function correctionMarks(): Extension {
  return [
    field,
    EditorView.domEventHandlers({
      click: (event, view) => {
        const index = clickedMark(event.target);
        if (index !== null) view.state.field(field).pick?.(index);
        // the caret goes where the person clicked, as always
        return false;
      },
    }),
  ];
}

/** The effect putting `ranges` (clamped to the text) as the marks; an empty list clears them. */
export function marksEffect(
  state: EditorState,
  ranges: readonly TextRange[],
  current: number | null,
  pick: (index: number) => void,
): StateEffect<Marks> {
  const length = state.doc.length;
  const decorations: Range<Decoration>[] = [];
  ranges.forEach((range, index) => {
    if (range.from < 0 || range.to > length || range.from > range.to) return;
    const now = index === current;
    decorations.push(
      range.from === range.to
        ? Decoration.widget({ widget: new InsertMark(index, now), side: 1, index }).range(
            range.from,
          )
        : Decoration.mark({
            class: markClass(now),
            attributes: { "data-correction-mark": String(index) },
            index,
          }).range(range.from, range.to),
    );
  });
  return setMarks.of(
    decorations.length === 0 ? NONE : { set: Decoration.set(decorations, true), pick },
  );
}

/** Index of the mark holding the caret (also right at its edges); `null`: none. */
export function markAt(state: EditorState): number | null {
  const head = state.selection.main.head;
  let found: number | null = null;
  state.field(field, false)?.set.between(head, head, (_from, _to, decoration) => {
    const index: unknown = decoration.spec.index;
    if (typeof index === "number") found = index;
    return false;
  });
  return found;
}
