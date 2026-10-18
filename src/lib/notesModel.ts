// The list of the Notes window as pure functions: which notes are pinned, how the rest are grouped by when they were last
// edited, which ones a filter keeps, and how the pinned ones are reordered. The window draws what these return.

import { foldChars } from "./text";
import type { NoteEntry } from "./types";

/** The groups the notes that are not pinned fall into, in the order the list shows them. */
export const GROUP_ORDER = ["today", "yesterday", "week", "month", "older"] as const;
export type GroupId = (typeof GROUP_ORDER)[number];

export type NoteGroup = { id: GroupId; notes: NoteEntry[] };
/** The list as drawn: the pinned rows, the groups of notes by time, and the loose files (by when they were opened). */
export type ArrangedNotes = { pinned: NoteEntry[]; groups: NoteGroup[]; files: NoteEntry[] };

const DAY_MS = 24 * 60 * 60 * 1000;

function startOfDay(ms: number): number {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/**
 * Which group a note edited at `modifiedMs` belongs to at `now`, by the calendar of the person's clock: today, yesterday,
 * earlier in the last week, earlier in the same month, or older.
 */
export function groupOf(modifiedMs: number, now: number): GroupId {
  // whole days between the two dates (a day of 23 or 25 hours at a clock change still counts as one)
  const days = Math.round((startOfDay(now) - startOfDay(modifiedMs)) / DAY_MS);
  if (days <= 0) return "today";
  if (days === 1) return "yesterday";
  if (days < 7) return "week";
  const [then, today] = [new Date(modifiedMs), new Date(now)];
  if (then.getFullYear() === today.getFullYear() && then.getMonth() === today.getMonth()) {
    return "month";
  }
  return "older";
}

/** The note that stays where it was in the list while it is being edited: a note moves up when you leave it. */
export type Frozen = { path: string; modifiedMs: number };

/**
 * The pinned rows first, in the order the backend gives them (the person's order), then the notes that are not pinned, newest
 * first in their groups, then the loose files that are not pinned in the order the backend gives them (the one opened last
 * first). A note that is `frozen` counts as last edited when it was frozen, so typing in it does not move it.
 */
export function arrange(
  entries: readonly NoteEntry[],
  now: number,
  frozen: Frozen | null = null,
): ArrangedNotes {
  const pinned = entries.filter((note) => note.pinned);
  const effective = (note: NoteEntry): number =>
    frozen?.path === note.path ? frozen.modifiedMs : note.modifiedMs;
  const rest = entries
    .filter((note) => !note.pinned && note.kind === "note")
    .sort((a, b) => effective(b) - effective(a) || a.path.localeCompare(b.path));
  const groups: NoteGroup[] = GROUP_ORDER.map((id) => ({ id, notes: [] }));
  for (const note of rest) {
    const group = groups.find((g) => g.id === groupOf(effective(note), now));
    group?.notes.push(note);
  }
  return {
    pinned,
    groups: groups.filter((group) => group.notes.length > 0),
    files: entries.filter((note) => !note.pinned && note.kind === "file"),
  };
}

/** Every row of an arrangement in the order the list draws it: the pinned ones, each group, then the files. */
export function drawnOrder(arranged: ArrangedNotes): NoteEntry[] {
  return [
    ...arranged.pinned,
    ...arranged.groups.flatMap((group) => group.notes),
    ...arranged.files,
  ];
}

/** Whether `note` matches what was typed in the filter box: every word is in its title, snippet or folder, or the search found it. */
export function matchesFilter(
  note: NoteEntry,
  query: string,
  found: ReadonlySet<string> | null = null,
): boolean {
  const words = foldChars(query.trim()).split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  if (found?.has(note.path)) return true;
  const text = foldChars(`${note.title}\n${note.snippet}\n${note.folder ?? ""}`);
  return words.every((word) => text.includes(word));
}

/** The notes the filter keeps, in their order. */
export function filterNotes(
  entries: readonly NoteEntry[],
  query: string,
  found: ReadonlySet<string> | null = null,
): NoteEntry[] {
  return entries.filter((note) => matchesFilter(note, query, found));
}

/** The order of the pinned notes after `path` moved one place up (`-1`) or down (`1`). The same order when it cannot. */
export function movePinned(order: readonly string[], path: string, direction: -1 | 1): string[] {
  const from = order.indexOf(path);
  const to = from + direction;
  if (from < 0 || to < 0 || to >= order.length) return [...order];
  const next = [...order];
  [next[from], next[to]] = [next[to], next[from]];
  return next;
}

/**
 * The order after `path` was dropped on `target`: it takes the place of `target`, and the ones between move over. The same
 * order when either is not pinned.
 */
export function dropPinned(order: readonly string[], path: string, target: string): string[] {
  const from = order.indexOf(path);
  const to = order.indexOf(target);
  if (from < 0 || to < 0 || from === to) return [...order];
  const next = [...order];
  next.splice(from, 1);
  next.splice(to, 0, path);
  return next;
}

/** The pinned note `Ctrl+1` to `Ctrl+9` jump to (`n` from 1), or null when there are fewer. */
export function pinnedAt(pinned: readonly NoteEntry[], n: number): NoteEntry | null {
  return n >= 1 && n <= 9 ? (pinned[n - 1] ?? null) : null;
}

/** The note after `path` in the list as drawn (`1`) or before it (`-1`); wraps around. Null when the list is empty. */
export function neighbour(
  shown: readonly string[],
  path: string | null,
  step: -1 | 1,
): string | null {
  if (shown.length === 0) return null;
  const at = path === null ? -1 : shown.indexOf(path);
  if (at < 0) return shown[step === 1 ? 0 : shown.length - 1];
  return shown[(at + step + shown.length) % shown.length];
}

/** The name of a file and of the folder it is in, from a path of the system (either kind of separator): `C:\work\plan\ask.md` is `ask.md` in `plan`. */
export function placeOf(path: string): { file: string; folder: string } {
  const parts = path.split(/[\\/]+/).filter((part) => part !== "");
  return { file: parts.at(-1) ?? "", folder: parts.at(-2) ?? "" };
}

/** How many characters of a folder label the list shows (the rest is in the tooltip). */
export const FOLDER_LABEL_CHARS = 34;

/**
 * A folder label (`~\AppData\Local\Temp\notes\loose\a`) cut in the middle when it is longer than `max` characters: the first part
 * and the last folders stay, since the last ones are what tell two files of the same name apart, and `…` stands for what is
 * left out (`~\…\loose\a`). A label that fits is left as it is.
 */
export function shortFolder(folder: string, max = FOLDER_LABEL_CHARS): string {
  if (folder.length <= max) return folder;
  const sep = folder.includes("\\") ? "\\" : "/";
  const parts = folder.split(sep);
  if (parts.length <= 2) return `…${folder.slice(folder.length - (max - 1))}`;
  const head = parts[0];
  let kept: string[] = [];
  for (let n = 1; n < parts.length; n++) {
    const tail = parts.slice(parts.length - n);
    const label = `${head}${sep}…${sep}${tail.join(sep)}`;
    if (label.length > max && n > 1) break;
    kept = tail;
  }
  const last = kept.join(sep);
  const label = `${head}${sep}…${sep}${last}`;
  // even the last folder alone is too long: its end is what is shown
  return label.length <= max ? label : `…${folder.slice(folder.length - (max - 1))}`;
}

/** Copy path, or show the file in the system file manager. */
export type PathAction = {
  id: "copyPath" | "show";
  /** `menu.copyPath`, or the show label a note and a loose file already use. */
  label: "menu.copyPath" | "menu.showInFileManager" | "notes.file.showInFolder";
};

/**
 * Copy path and Show in the file manager for a note or a loose file.
 * Copy path stays while a path is known. A missing file is not shown: there is nothing to select.
 * A note uses the project wording ("Show in file manager"); a loose file keeps "Show in folder".
 */
export function pathActions(kind: "note" | "file", missing: boolean): PathAction[] {
  const actions: PathAction[] = [{ id: "copyPath", label: "menu.copyPath" }];
  if (!missing) {
    actions.push({
      id: "show",
      label: kind === "note" ? "menu.showInFileManager" : "notes.file.showInFolder",
    });
  }
  return actions;
}

/** The place of a loose file as a tooltip shows it: its folder label and its name, with the separator the label uses. */
export function fullPlace(folder: string | null, name: string): string {
  if (!folder) return name;
  return `${folder}${folder.includes("\\") ? "\\" : "/"}${name}`;
}

/** What the status bar says about saving: nothing, "Unsaved changes", "Saving...", "Saved", or the note a loose file shows for a moment. */
export type SaveStatus = "none" | "unsaved" | "saving" | "saved" | "note";

/** What `saveStatus` looks at. */
export type SaveFacts = {
  /** A note or a file is open (a file that is not found counts as not). */
  shown: boolean;
  /** The window can only read: nothing is ever written, so nothing is said about saving. */
  readOnly: boolean;
  /** The open document is a loose file, written only when asked. */
  isFile: boolean;
  dirty: boolean;
  saving: boolean;
  /** A save is taking longer than a second. */
  savingSlow: boolean;
};

/**
 * "Saved" when the note is on disk; "Saving..." only when a save takes longer than a second; nothing while typing. A loose file
 * is written when asked: it says "Unsaved changes" until then. A window that can only read says nothing: it never saves.
 */
export function saveStatus(facts: SaveFacts): SaveStatus {
  if (!facts.shown || facts.readOnly) return "none";
  if (facts.isFile) return facts.dirty ? "unsaved" : "note";
  if (facts.savingSlow) return "saving";
  return facts.dirty || facts.saving ? "none" : "saved";
}
