// The list of the Notes window as pure functions: which notes are pinned, how the rest are grouped by when they were last
// edited, which ones a filter keeps, and how the pinned ones are reordered. The window draws what these return.

import type { NoteEntry } from "../../api";

/** The groups the notes that are not pinned fall into, in the order the list shows them. */
export const GROUP_ORDER = ["today", "yesterday", "week", "month", "older"] as const;
export type GroupId = (typeof GROUP_ORDER)[number];

export type NoteGroup = { id: GroupId; notes: NoteEntry[] };
/** The list as drawn: the pinned rows, the groups of notes by time, and the loose files (by when they were opened). */
export type Arranged = { pinned: NoteEntry[]; groups: NoteGroup[]; files: NoteEntry[] };

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
 * The pinned rows first, in the order the backend gives them (the person's order), then the notes that are not pinned,
 * newest first in their groups, then the loose files that are not pinned in the order the backend gives them (the one
 * opened last first). A note that is `frozen` counts as last edited when it was frozen, so typing in it does not move it.
 */
export function arrange(
  entries: readonly NoteEntry[],
  now: number,
  frozen: Frozen | null = null,
): Arranged {
  const effective = (note: NoteEntry): number =>
    frozen?.path === note.path ? frozen.modifiedMs : note.modifiedMs;
  const rest = entries
    .filter((note) => !note.pinned && note.kind === "note")
    .sort((a, b) => effective(b) - effective(a) || a.path.localeCompare(b.path));
  const groups: NoteGroup[] = GROUP_ORDER.map((id) => ({ id, notes: [] }));
  for (const note of rest) {
    groups.find((g) => g.id === groupOf(effective(note), now))?.notes.push(note);
  }
  return {
    pinned: entries.filter((note) => note.pinned),
    groups: groups.filter((group) => group.notes.length > 0),
    files: entries.filter((note) => !note.pinned && note.kind === "file"),
  };
}

/** Every row of an arrangement in the order the list draws it. */
export function drawnOrder(arranged: Arranged): NoteEntry[] {
  return [
    ...arranged.pinned,
    ...arranged.groups.flatMap((group) => group.notes),
    ...arranged.files,
  ];
}

/** Accents and case taken off, so "Café" is found by "cafe". */
export function foldChars(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

/** Whether `note` matches what was typed in the filter box: every word is in its title, snippet, name or folder. */
export function matchesFilter(note: NoteEntry, query: string): boolean {
  const words = foldChars(query.trim()).split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const text = foldChars(`${note.title}\n${note.snippet}\n${note.name}\n${note.folder ?? ""}`);
  return words.every((word) => text.includes(word));
}

/** The notes the filter keeps, in their order. */
export function filterNotes(entries: readonly NoteEntry[], query: string): NoteEntry[] {
  return entries.filter((note) => matchesFilter(note, query));
}

/** The order of the pinned notes after `path` was dropped on `target`: it takes the place of `target`. */
export function dropPinned(order: readonly string[], path: string, target: string): string[] {
  const from = order.indexOf(path);
  const to = order.indexOf(target);
  if (from < 0 || to < 0 || from === to) return [...order];
  const next = [...order];
  next.splice(from, 1);
  next.splice(to, 0, path);
  return next;
}

/** The order after `path` moved one place up (`-1`) or down (`1`). The same order when it cannot. */
export function movePinned(order: readonly string[], path: string, direction: -1 | 1): string[] {
  const from = order.indexOf(path);
  const to = from + direction;
  if (from < 0 || to < 0 || to >= order.length) return [...order];
  const next = [...order];
  const moved = next[from];
  const other = next[to];
  if (moved === undefined || other === undefined) return [...order];
  next[from] = other;
  next[to] = moved;
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
  if (at < 0) return shown[step === 1 ? 0 : shown.length - 1] ?? null;
  return shown[(at + step + shown.length) % shown.length] ?? null;
}

/** The name of a file and of the folder it is in, from a path of the system (either kind of separator). */
export function placeOf(path: string): { file: string; folder: string } {
  const parts = path.split(/[\\/]+/).filter((part) => part !== "");
  return { file: parts.at(-1) ?? "", folder: parts.at(-2) ?? "" };
}

/** A name for a copy of a title to show in a tooltip or a toast: the title, else the file name without extension. */
export function shownName(note: Pick<NoteEntry, "title" | "name">): string {
  if (note.title !== "") return note.title;
  const dot = note.name.lastIndexOf(".");
  return dot > 0 ? note.name.slice(0, dot) : note.name;
}

/** The folder of a loose file cut in the middle when it is long: the first part and the last folders stay. */
export function shortFolder(folder: string, max = 34): string {
  if (folder.length <= max) return folder;
  const sep = folder.includes("\\") ? "\\" : "/";
  const parts = folder.split(sep);
  if (parts.length <= 2) return `…${folder.slice(folder.length - (max - 1))}`;
  const head = parts[0] ?? "";
  let kept: string[] = [];
  for (let n = 1; n < parts.length; n++) {
    const tail = parts.slice(parts.length - n);
    const label = `${head}${sep}…${sep}${tail.join(sep)}`;
    if (label.length > max && n > 1) break;
    kept = tail;
  }
  const label = `${head}${sep}…${sep}${kept.join(sep)}`;
  return label.length <= max ? label : `…${folder.slice(folder.length - (max - 1))}`;
}

/** Whether the row shows a mark, in the order the row draws them. */
export type RowMarks = {
  recovered: boolean;
  unsaved: boolean;
  changed: boolean;
  gone: boolean;
  shared: boolean;
  copied: boolean;
};

export function marksOf(note: NoteEntry): RowMarks {
  return {
    recovered: note.recovered,
    unsaved: note.state === "unsaved",
    changed: note.state === "changedOnDisk",
    gone: note.state === "gone",
    shared: note.shared,
    copied: note.copied,
  };
}

/** When a note was edited as a row says it: the time today, the weekday this week, the date otherwise. */
export function timeLabel(modifiedMs: number, now: number): string {
  const d = new Date(modifiedMs);
  switch (groupOf(modifiedMs, now)) {
    case "today":
      return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
    case "yesterday":
    case "week":
      return d.toLocaleDateString(undefined, { weekday: "short" });
    case "month":
      return d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
    case "older": {
      const sameYear = d.getFullYear() === new Date(now).getFullYear();
      return d.toLocaleDateString(
        undefined,
        sameYear
          ? { day: "numeric", month: "short" }
          : { day: "numeric", month: "short", year: "numeric" },
      );
    }
    default:
      return "";
  }
}
