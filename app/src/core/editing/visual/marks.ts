// Correction marks in Edit mode (session `marks`): text offsets -> document positions (`docRange`), an inline
// decoration per range found (an empty range: a caret-width widget), the current one with its own class; set by a
// meta transaction kept out of the undo history, mapped through every change until the next set. A range not found
// in its block (markup inside, a locked block) gets no mark. A click on one picks it and leaves the caret alone.
// Classes + look: `../marks.ts`, `../marks.css`.

import { Plugin, PluginKey, type EditorState } from "@milkdown/kit/prose/state";
import { Decoration, DecorationSet, type EditorView } from "@milkdown/kit/prose/view";
import { clickedMark, markClass } from "../marks";
import type { TextRange } from "../session.svelte";
import type { Composed } from "./compose";
import { docRange } from "./edits";

type Marks = { set: DecorationSet; pick: ((index: number) => void) | null };

const NONE: Marks = { set: DecorationSet.empty, pick: null };
const marksKey = new PluginKey<Marks>("PM_CORRECTION_MARKS");

function isMarks(value: unknown): value is Marks {
  return typeof value === "object" && value !== null && "set" in value && "pick" in value;
}

export function marksPlugin(): Plugin<Marks> {
  return new Plugin<Marks>({
    key: marksKey,
    state: {
      init: () => NONE,
      apply: (tr, value) => {
        const meta: unknown = tr.getMeta(marksKey);
        if (isMarks(meta)) return meta;
        return tr.docChanged ? { ...value, set: value.set.map(tr.mapping, tr.doc) } : value;
      },
    },
    props: {
      decorations: (state) => marksKey.getState(state)?.set ?? null,
      handleDOMEvents: {
        click: (view, event) => {
          const index = clickedMark(event.target);
          if (index !== null) marksKey.getState(view.state)?.pick?.(index);
          // the caret goes where the person clicked, as always
          return false;
        },
      },
    },
  });
}

function insertMark(index: number, current: boolean): HTMLElement {
  const span = document.createElement("span");
  span.className = markClass(current, true);
  span.setAttribute("data-correction-mark", String(index));
  span.setAttribute("aria-hidden", "true");
  return span;
}

/** Marks `ranges` (offsets of `composed.text`); an empty list clears them. */
export function setMarks(
  view: EditorView,
  composed: Composed,
  ranges: readonly TextRange[],
  current: number | null,
  pick: (index: number) => void,
): void {
  const doc = view.state.doc;
  const decorations: Decoration[] = [];
  ranges.forEach((range, index) => {
    const found = docRange(doc, composed, range);
    if (found === null) return;
    const now = index === current;
    decorations.push(
      found.from === found.to
        ? Decoration.widget(found.from, () => insertMark(index, now), {
            side: 1,
            index,
            key: `correction-${index}-${now}`,
            ignoreSelection: true,
          })
        : Decoration.inline(
            found.from,
            found.to,
            { class: markClass(now), "data-correction-mark": String(index) },
            { index },
          ),
    );
  });
  const marks =
    decorations.length === 0 ? NONE : { set: DecorationSet.create(doc, decorations), pick };
  view.dispatch(view.state.tr.setMeta(marksKey, marks).setMeta("addToHistory", false));
}

/** Index of the mark holding the caret (also right at its edges); `null`: none. */
export function markAt(state: EditorState): number | null {
  const head = state.selection.head;
  const found = marksKey.getState(state)?.set.find(head, head) ?? [];
  const index: unknown = found[0]?.spec.index;
  return typeof index === "number" ? index : null;
}
