// The language dialog of a window. Who asked for it and what follows its confirm: one dialog at a time; the two
// languages picked in it are the window's only once confirmed (`Languages.choose` | `from`), a closed dialog changes
// nothing. What it lists: every language offered, by name in the app's language, the recently used ones first; while
// the person types, only the languages whose name here, own name, English name or tag match, best match first.

import { language, type MessageKey } from "../../core/i18n";
import { fuzzyMatch } from "../../core/search/fuzzy";
import { englishName, namesIn, offered, ownName, sameTag } from "./languages";
import { translation, type Languages } from "./store.svelte";

/** A line saying what the confirm does: as it is, or made for the language picked to translate into (its tag; asked
 *  again when the pick changes, nothing shown while none is picked). */
export type LanguageNote = string | ((target: string) => string);

/** What an opener asks of the dialog. */
export type LanguageAsk = {
  /** What the confirm button says (default: "Save"). */
  confirm?: MessageKey;
  /** Shown above the buttons ("Creates readme.de.md next to this file"). */
  note?: LanguageNote;
  /** Runs after the confirm: the choice is the window's by then and the dialog closed. */
  then?: () => void;
};

export type LanguageDialogState =
  | { status: "closed" }
  | { status: "open"; confirm: MessageKey; note: LanguageNote | null; then: (() => void) | null };

export class LanguageDialogStore {
  state = $state.raw<LanguageDialogState>({ status: "closed" });
  /** The languages the dialog starts from and its confirm changes. */
  readonly languages: Languages;

  constructor(languages: Languages) {
    this.languages = languages;
  }

  /** Opens the dialog for `request`. While it is open: nothing (the open one keeps its opener). */
  ask(request: LanguageAsk = {}): void {
    if (this.state.status === "open") return;
    this.state = {
      status: "open",
      confirm: request.confirm ?? "translation.language.save",
      note: request.note ?? null,
      then: request.then ?? null,
    };
  }

  /** The person confirmed: from now on the window translates into `target`, from `source` (`null`: read off the
   *  text); the dialog goes, then the opener's `then` runs. */
  confirm(target: string, source: string | null): void {
    const state = this.state;
    if (state.status !== "open") return;
    // the target first: a source equal to the target replaced would not be taken
    void this.languages.choose(target);
    this.languages.from(source);
    this.close();
    state.then?.();
  }

  /** Closes the dialog: nothing changes, nothing follows. */
  close(): void {
    this.state = { status: "closed" };
  }
}

/** The language dialog of this window. */
export const languageDialog = new LanguageDialogStore(translation.languages);

/** Which of the two languages the list is for: the one translated into | from. */
export type Side = "to" | "from";

/** A language as the dialog lists it. */
export type Listed = {
  tag: string;
  /** Its name in the app's language. */
  name: string;
  /** Its name in the language itself, where that reads differently. */
  own: string | null;
  /** Its English name: found by it in a window of any language. */
  english: string;
};

/** Recently used languages the list starts with, at most. */
export const RECENT_SHOWN = 5;

/** Every language offered, named in `locale` and in the order of that language. */
export function listed(locale: string = language.locale): Listed[] {
  const nameHere = namesIn(locale);
  const order = new Intl.Collator(locale);
  return offered()
    .map((tag) => {
      const name = nameHere(tag);
      const own = ownName(tag);
      return { tag, name, own: own === name ? null : own, english: englishName(tag) ?? tag };
    })
    .sort((a, b) => order.compare(a.name, b.name));
}

/** A row of the list: "detect" (From only: the language is read off the text), or a language with the characters
 *  of its names the typed text matched and whether it can be picked (`off`: it is the language translated into). */
export type Row =
  | { kind: "detect"; key: string }
  | {
      kind: "language";
      /** Unique in the list: a recently used language is listed twice. */
      key: string;
      language: Listed;
      nameMarks: readonly number[];
      ownMarks: readonly number[];
      off: boolean;
    };

/** Rows under one title (`null`: no title, the row stands before the groups). */
export type Group = { title: "recent" | "all" | null; rows: Row[] };

export type Listing = {
  side: Side;
  /** What the person typed. */
  query: string;
  /** The languages, in the order they are listed (`listed`). */
  list: readonly Listed[];
  /** Tags of the languages used before, newest first. */
  recent: readonly string[];
  /** The language picked to translate into: never one to translate from. */
  target: string | null;
  /** The words of the "detect" row: it is found by them. */
  detect: string;
};

/** The languages `query` finds, best match first (equal matches stay in the list's order), each with the characters
 *  of its two shown names that matched. */
function found(
  list: readonly Listed[],
  query: string,
): { entry: Listed; nameMarks: readonly number[]; ownMarks: readonly number[] }[] {
  const hits = list.flatMap((entry) => {
    const name = fuzzyMatch(query, entry.name);
    const own = entry.own === null ? null : fuzzyMatch(query, entry.own);
    const scores = [name, own, fuzzyMatch(query, entry.english), fuzzyMatch(query, entry.tag)]
      .filter((match) => match !== null)
      .map((match) => match.score);
    return scores.length === 0
      ? []
      : [
          {
            entry,
            score: Math.max(...scores),
            nameMarks: name?.positions ?? [],
            ownMarks: own?.positions ?? [],
          },
        ];
  });
  return hits.sort((a, b) => b.score - a.score);
}

/** What the list shows, group by group. */
export function groupsOf({ side, query, list, recent, target, detect }: Listing): Group[] {
  const typed = query.trim() !== "";
  const row = (
    group: "recent" | "all",
    entry: Listed,
    nameMarks: readonly number[] = [],
    ownMarks: readonly number[] = [],
  ): Row => ({
    kind: "language",
    key: `${group}-${entry.tag}`,
    language: entry,
    nameMarks,
    ownMarks,
    off: side === "from" && target !== null && sameTag(entry.tag, target),
  });
  const groups: Group[] = [];
  if (side === "from" && (!typed || fuzzyMatch(query, detect) !== null))
    groups.push({ title: null, rows: [{ kind: "detect", key: "detect" }] });
  if (typed) {
    const rows = found(list, query).map((hit) =>
      row("all", hit.entry, hit.nameMarks, hit.ownMarks),
    );
    if (rows.length > 0) groups.push({ title: "all", rows });
    return groups;
  }
  const used = recent.slice(0, RECENT_SHOWN).flatMap((tag) => {
    const entry = list.find((known) => sameTag(known.tag, tag));
    return entry === undefined ? [] : [row("recent", entry)];
  });
  if (used.length > 0) groups.push({ title: "recent", rows: used });
  groups.push({ title: "all", rows: list.map((entry) => row("all", entry)) });
  return groups;
}
