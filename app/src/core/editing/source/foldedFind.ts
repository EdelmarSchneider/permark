// Find in Source ignores case with the fold of find in page (`foldCase`). CodeMirror's own search lowercases each
// character after decomposing it, so `istanbul` missed `İstanbul` (`I` + a combining dot) and `ışık` missed `Işık`.
// A plain query that ignores case is swapped for one that runs CodeMirror's case-sensitive search over the folded
// text: `foldCase` keeps every UTF-16 unit in its place, so an offset in the folded text is one in the document.

import { SearchQuery, setSearchQuery } from "@codemirror/search";
import { EditorState, Text, type Extension } from "@codemirror/state";
import { foldCase } from "../../doc/find";

type Match = { from: number; to: number };

/** One fold per document (a document never changes: an edit makes a new one). */
const folds = new WeakMap<Text, Text>();

function folded(doc: Text): Text {
  let text = folds.get(doc);
  if (text === undefined) {
    const plain = doc.toString();
    const lower = foldCase(plain);
    // a fold of another length would shift the offsets; then the text is searched as it is
    text = lower.length === plain.length ? Text.of(lower.split("\n")) : doc;
    folds.set(doc, text);
  }
  return text;
}

/** `\n`, `\r`, `\t` and `\\` in a query that is not literal, read as CodeMirror reads them. */
function unquote(text: string): string {
  return text.replace(/\\([nrt\\])/g, (_, ch: string) =>
    ch === "n" ? "\n" : ch === "r" ? "\r" : ch === "t" ? "\t" : "\\",
  );
}

/** The matches of a folded query, in the shape CodeMirror's search state asks its query for. */
class FoldedMatches {
  readonly spec: SearchQuery;
  readonly #inner: SearchQuery;
  readonly #length: number;

  constructor(spec: SearchQuery) {
    this.spec = spec;
    const needle = foldCase(spec.literal ? spec.search : unquote(spec.search));
    this.#length = needle.length;
    this.#inner = new SearchQuery({
      search: needle,
      caseSensitive: true,
      literal: true,
      wholeWord: spec.wholeWord,
    });
  }

  #cursor(state: EditorState, from: number, to: number): Iterator<Match> {
    return this.#inner.getCursor(folded(state.doc), from, to);
  }

  #first(state: EditorState, from: number, to: number): Match | null {
    const next = this.#cursor(state, from, to).next();
    return next.done === true ? null : next.value;
  }

  #last(state: EditorState, from: number, to: number): Match | null {
    const cursor = this.#cursor(state, from, to);
    let last: Match | null = null;
    for (let next = cursor.next(); next.done !== true; next = cursor.next()) last = next.value;
    return last;
  }

  /** The match after the current one, around to the start past the end (`null`: no other). */
  nextMatch(state: EditorState, curFrom: number, curTo: number): Match | null {
    const end = state.doc.length;
    const found =
      this.#first(state, curTo, end) ??
      this.#first(state, 0, Math.min(end, curFrom + this.#length));
    return found === null || (found.from === curFrom && found.to === curTo) ? null : found;
  }

  /** The match before the current one, around to the end past the start (`null`: no other). */
  prevMatch(state: EditorState, curFrom: number, curTo: number): Match | null {
    const found =
      this.#last(state, 0, curFrom) ??
      this.#last(state, Math.max(0, curTo - this.#length), state.doc.length);
    return found !== null && (found.from !== curFrom || found.to !== curTo) ? found : null;
  }

  getReplacement(): string {
    return this.spec.literal ? this.spec.replace : unquote(this.spec.replace);
  }

  /** Every match, `null` past `limit`. */
  matchAll(state: EditorState, limit: number): Match[] | null {
    const cursor = this.#cursor(state, 0, state.doc.length);
    const all: Match[] = [];
    for (let next = cursor.next(); next.done !== true; next = cursor.next()) {
      if (all.length >= limit) return null;
      all.push(next.value);
    }
    return all;
  }

  highlight(
    state: EditorState,
    from: number,
    to: number,
    add: (from: number, to: number) => void,
  ): void {
    const cursor = this.#cursor(
      state,
      Math.max(0, from - this.#length),
      Math.min(to + this.#length, state.doc.length),
    );
    for (let next = cursor.next(); next.done !== true; next = cursor.next()) {
      add(next.value.from, next.value.to);
    }
  }
}

/** A plain query that ignores case, searched in the folded text. */
class FoldedQuery extends SearchQuery {
  /** Where CodeMirror's search state takes the matches of a query from (its own queries have it; not in its types). */
  create(): FoldedMatches {
    return new FoldedMatches(this);
  }
}

/** Each plain query that ignores case, set in a transaction, is followed by its folded twin: the last query set is the
 *  one searched, and the panel shows the same words. Queries with a filter of their own stay as they are. */
export const foldedFind: Extension = EditorState.transactionExtender.of((tr) => {
  let query: SearchQuery | null = null;
  for (const effect of tr.effects) if (effect.is(setSearchQuery)) query = effect.value;
  if (
    query === null ||
    query instanceof FoldedQuery ||
    query.caseSensitive ||
    query.regexp ||
    query.test !== undefined
  ) {
    return null;
  }
  const twin = new FoldedQuery({
    search: query.search,
    literal: query.literal,
    replace: query.replace,
    wholeWord: query.wholeWord,
  });
  return { effects: setSearchQuery.of(twin) };
});
