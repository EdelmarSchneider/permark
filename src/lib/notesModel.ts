// The list of the Notes window as pure functions: which notes are pinned, how the rest are grouped by when they were last
// edited, which ones a filter keeps, and how the pinned ones are reordered. The window draws what these return.

import { foldChars } from "./text";
import type { NoteEntry } from "./types";

/** The groups the notes that are not pinned fall into, in the order the list shows them. */
export const GROUP_ORDER = ["today", "yesterday", "week", "month", "older"] as const;
export type GroupId = (typeof GROUP_ORDER)[number];

export type NoteGroup = { id: GroupId; notes: NoteEntry[] };
export type ArrangedNotes = { pinned: NoteEntry[]; groups: NoteGroup[] };

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
 * The pinned notes first, in the order the backend gives them (the person's order), then the others newest first in their
 * groups. A note that is `frozen` counts as last edited when it was frozen, so typing in it does not move it.
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
    .filter((note) => !note.pinned)
    .sort((a, b) => effective(b) - effective(a) || a.path.localeCompare(b.path));
  const groups: NoteGroup[] = GROUP_ORDER.map((id) => ({ id, notes: [] }));
  for (const note of rest) {
    const group = groups.find((g) => g.id === groupOf(effective(note), now));
    group?.notes.push(note);
  }
  return { pinned, groups: groups.filter((group) => group.notes.length > 0) };
}

/** Whether `note` matches what was typed in the filter box: every word is in its title or snippet, or the search found it. */
export function matchesFilter(
  note: NoteEntry,
  query: string,
  found: ReadonlySet<string> | null = null,
): boolean {
  const words = foldChars(query.trim()).split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  if (found?.has(note.path)) return true;
  const text = foldChars(`${note.title}\n${note.snippet}`);
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
