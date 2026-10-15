// Review notes as lists (docs/design/v0.4.md section 3): what the Notes tab groups and what the margin shows. Pure; the
// store keeps the notes and the components draw them.

import type { Note } from "./types";

export type NoteGroup = { path: string; notes: Note[] };

/** The longest quote a row shows, in characters. */
export const EXCERPT_CHARS = 140;

const byLine = (a: Note, b: Note): number =>
  a.line - b.line || a.createdMs - b.createdMs || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);

const byPath = (a: string, b: string): number => {
  const [x, y] = [a.toLowerCase(), b.toLowerCase()];
  return x < y ? -1 : x > y ? 1 : a < b ? -1 : a > b ? 1 : 0;
};

/** The notes still open, grouped by document: documents in path order, each document's notes by line. */
export function openByDocument(notes: readonly Note[]): NoteGroup[] {
  const groups = new Map<string, Note[]>();
  for (const note of notes) {
    if (note.done) continue;
    const list = groups.get(note.path);
    if (list) list.push(note);
    else groups.set(note.path, [note]);
  }
  return [...groups]
    .sort(([a], [b]) => byPath(a, b))
    .map(([path, list]) => ({ path, notes: list.sort(byLine) }));
}

/** The notes marked done, the latest first. */
export function doneNotes(notes: readonly Note[]): Note[] {
  return notes
    .filter((note) => note.done)
    .sort(
      (a, b) =>
        (b.doneMs ?? b.createdMs) - (a.doneMs ?? a.createdMs) ||
        byPath(a.path, b.path) ||
        byLine(a, b),
    );
}

/** How many notes are open. */
export function openCount(notes: readonly Note[]): number {
  return notes.reduce((count, note) => count + (note.done ? 0 : 1), 0);
}

/** The notes that get a bar in the margin of a document: open ones whose passage is where the note says. */
export function barNotes(notes: readonly Note[], path: string | null): Note[] {
  if (path === null) return [];
  return notes.filter((n) => n.path === path && !n.done && n.state === "anchored").sort(byLine);
}

/** Whether a document has notes that are not done (the file lists and tabs can say so). */
export function hasOpenNotes(notes: readonly Note[], path: string): boolean {
  return notes.some((note) => note.path === path && !note.done);
}

/** A quote shortened to fit a row: spaces collapsed, cut at a word where it can be, with an ellipsis. */
export function excerpt(text: string, max = EXCERPT_CHARS): string {
  const flat = text.replace(/\s+/g, " ").trim();
  if (flat.length <= max) return flat;
  const cut = flat.slice(0, max);
  const space = cut.lastIndexOf(" ");
  return `${(space > max * 0.6 ? cut.slice(0, space) : cut).trimEnd()}…`;
}

/** The headings above a passage as a trail ("Plan › Rollout"). */
export function trail(headingPath: readonly string[]): string {
  return headingPath.join(" › ");
}

/** The document a note is on, as a group title: its file name, and the folder it is in. */
export function documentTitle(path: string): { name: string; folder: string } {
  const slash = path.lastIndexOf("/");
  return slash < 0
    ? { name: path, folder: "" }
    : { name: path.slice(slash + 1), folder: path.slice(0, slash) };
}
