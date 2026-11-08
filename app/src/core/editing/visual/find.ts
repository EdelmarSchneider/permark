// Find in page in Edit: the matches in the text of the editor's text blocks, painted as decorations (the document
// never changes), set by a meta transaction kept out of the undo history and mapped through edits until the next
// search; a change of the text tells the find bar to search again. What atoms draw (blocks kept as written, the front
// matter card, inline HTML | formulas) is not the editor's text and is not searched. Look: `doc/markdown.css`.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { Plugin, PluginKey, TextSelection } from "@milkdown/kit/prose/state";
import { Decoration, DecorationSet, type EditorView } from "@milkdown/kit/prose/view";
import { FIND_LIMIT, offsetsOf, scrollIntoPane, type Finder } from "../../doc/find";

/** A match: document positions. */
type Match = { from: number; to: number };

const findKey = new PluginKey<DecorationSet>("PM_FIND");

/** Who waits for a change of each editor's text (the find bar open on it). */
const watchers = new WeakMap<EditorView, Set<() => void>>();

export function findPlugin(): Plugin<DecorationSet> {
  return new Plugin<DecorationSet>({
    key: findKey,
    state: {
      init: () => DecorationSet.empty,
      apply: (tr, value) => {
        const meta: unknown = tr.getMeta(findKey);
        if (meta instanceof DecorationSet) return meta;
        return tr.docChanged ? value.map(tr.mapping, tr.doc) : value;
      },
    },
    props: { decorations: (state) => findKey.getState(state) ?? null },
    view: (view) => {
      // one search per frame however fast the person types; never inside the update that told of the change
      let frame = 0;
      return {
        update: (updated, previous) => {
          if (updated.state.doc === previous.doc || frame !== 0) return;
          if ((watchers.get(view)?.size ?? 0) === 0) return;
          frame = requestAnimationFrame(() => {
            frame = 0;
            for (const again of watchers.get(view) ?? []) again();
          });
        },
        destroy: () => {
          if (frame !== 0) cancelAnimationFrame(frame);
          watchers.delete(view);
        },
      };
    },
  });
}

/**
 * Every match of `query` (case-insensitive) in the text blocks of `doc`, in document order. A match stays within a run
 * of text: an inline atom between two words ends the run.
 */
export function matchesIn(
  doc: PMNode,
  query: string,
  limit = FIND_LIMIT,
): { matches: Match[]; capped: boolean } {
  const needle = query.toLowerCase();
  const matches: Match[] = [];
  let capped = false;
  if (needle === "") return { matches, capped };
  doc.descendants((node, pos) => {
    if (capped) return false;
    if (!node.isTextblock) return true;
    let run = "";
    let start = 0;
    const flush = (): void => {
      for (const at of offsetsOf(run, needle)) {
        if (matches.length >= limit) {
          capped = true;
          break;
        }
        matches.push({ from: start + at, to: start + at + needle.length });
      }
      run = "";
    };
    node.forEach((child, offset) => {
      if (!child.isText) {
        flush();
        return;
      }
      if (run === "") start = pos + 1 + offset;
      run += child.text ?? "";
    });
    flush();
    return false;
  });
  return { matches, capped };
}

function paintMatches(view: EditorView, matches: readonly Match[], current: number): void {
  const set =
    matches.length === 0
      ? DecorationSet.empty
      : DecorationSet.create(
          view.state.doc,
          matches.map((match, index) =>
            Decoration.inline(match.from, match.to, {
              class: index === current ? "find-match find-match--current" : "find-match",
            }),
          ),
        );
  view.dispatch(view.state.tr.setMeta(findKey, set).setMeta("addToHistory", false));
}

/** Find over the editor `view()` gives (`null`: it is gone), which scrolls in `pane`. */
export function editFinder(view: () => EditorView | null, pane: HTMLElement): Finder {
  let matches: readonly Match[] = [];
  return {
    search: (query) => {
      const now = view();
      const found = now ? matchesIn(now.state.doc, query) : { matches: [], capped: false };
      matches = found.matches;
      return { total: matches.length, capped: found.capped };
    },
    show: (current, scroll) => {
      const now = view();
      if (!now) return;
      paintMatches(now, matches, current);
      const match = matches[current];
      if (scroll && match) scrollIntoPane(pane, now.coordsAtPos(match.from));
    },
    clear: () => {
      matches = [];
      const now = view();
      if (now && findKey.getState(now.state) !== DecorationSet.empty) paintMatches(now, [], 0);
    },
    watch: (again) => {
      const now = view();
      if (!now) return () => {};
      const own = watchers.get(now) ?? new Set<() => void>();
      watchers.set(now, own);
      own.add(again);
      return () => own.delete(again);
    },
    done: (current) => {
      const now = view();
      if (!now) return;
      const match = current === null ? undefined : matches[current];
      // a match from before an edit the next search has not caught up with yet may lie past the end
      if (match && match.to <= now.state.doc.content.size) {
        const selection = TextSelection.create(now.state.doc, match.from, match.to);
        now.dispatch(now.state.tr.setSelection(selection).scrollIntoView());
      }
      now.focus();
    },
  };
}
