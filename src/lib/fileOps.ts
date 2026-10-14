// File operations in the Explorer (docs/design/v0.2.md section 17): the pure rules behind rename, move by drag and
// drop, delete, multi-select and undo. The stores and components call these; the backend checks the same rules
// again, so nothing here is a security boundary.

import { t, tn, type MessageKey } from "./i18n";
import { basename, dirname, splitWorktree } from "./paths";
import type { LinkUpdate, Move, MovePreview, SkippedFile } from "./types";

// ---- the setting ---------------------------------------------------------------------------------

export const LINK_UPDATES: readonly LinkUpdate[] = ["ask", "always", "never"];
export const DEFAULT_LINK_UPDATE: LinkUpdate = "ask";

/** A stored value, or the default when it is missing or one this version does not know. */
export function normalizeLinkUpdate(value: unknown): LinkUpdate {
  return LINK_UPDATES.find((v) => v === value) ?? DEFAULT_LINK_UPDATE;
}

/**
 * What to do with the links once the preview is known: apply the move and rewrite them, move without rewriting, or ask.
 * A move that changes no link needs no question.
 */
export function linkDecision(setting: LinkUpdate, preview: MovePreview): "update" | "skip" | "ask" {
  if (setting === "never") return "skip";
  if (preview.linkCount === 0) return "update";
  return setting === "always" ? "update" : "ask";
}

/** "Never" does not even look at the documents. */
export function shouldScanLinks(setting: LinkUpdate): boolean {
  return setting !== "never";
}

// ---- paths ---------------------------------------------------------------------------------------

/** A name inside a folder ("" is the project root). */
export function joinPath(folder: string, name: string): string {
  return folder === "" ? name : `${folder}/${name}`;
}

/** The path of an entry after it is renamed: same folder, new name. */
export function renamedPath(path: string, newName: string): string {
  return joinPath(dirname(path), newName);
}

/** The root a path belongs to: "" for the project itself, `@wt/<name>` for a worktree. */
export function rootOf(path: string): string {
  const { worktree } = splitWorktree(path);
  return worktree === null ? "" : `@wt/${worktree}`;
}

/** Whether `path` is `folder` or somewhere below it. */
export function isInside(path: string, folder: string): boolean {
  return path === folder || path.startsWith(`${folder}/`);
}

/** The paths that are not inside another one of the list (a folder takes its contents along), without repeats. */
export function topLevelOnly(paths: readonly string[]): string[] {
  const unique = [...new Set(paths)];
  return unique.filter((p) => !unique.some((other) => other !== p && isInside(p, other)));
}

/** Whether the path is a root the Explorer cannot rename, move or delete (the project's own, or a worktree's). */
export function isRootPath(path: string): boolean {
  return path === "" || (path.startsWith("@wt/") && splitWorktree(path).rel === "");
}

// ---- moves -----------------------------------------------------------------------------------------

/** The moves that put `sources` into `folder`, each keeping its name. */
export function movesFor(sources: readonly string[], folder: string): Move[] {
  return topLevelOnly(sources).map((from) => ({ from, to: joinPath(folder, basename(from)) }));
}

export type DropProblem = "notFolder" | "inItself" | "samePlace" | "otherRoot" | "root";

/**
 * Why `sources` cannot be dropped into `folder`, or null when they can. A folder cannot go into itself or one of its
 * own folders, an item dropped where it already is does nothing, and nothing crosses between the project and a
 * worktree (they are separate roots).
 */
export function dropProblem(sources: readonly string[], folder: string): DropProblem | null {
  const items = topLevelOnly(sources);
  if (items.length === 0) return "notFolder";
  if (items.some(isRootPath)) return "root";
  if (items.some((s) => isInside(folder, s))) return "inItself";
  if (items.some((s) => rootOf(s) !== rootOf(folder))) return "otherRoot";
  if (items.every((s) => dirname(s) === folder)) return "samePlace";
  return null;
}

/** The folder a drop on a row lands in: the folder itself, or the folder that holds a file. */
export function dropFolder(row: { path: string; isDir: boolean }): string {
  return row.isDir ? row.path : dirname(row.path);
}

/** Where `path` ends up after `moves`: itself, or the moved item it is (or is inside of). */
export function mapPath(path: string, moves: readonly Move[]): string {
  let best: Move | null = null;
  for (const m of moves) {
    if (isInside(path, m.from) && (best === null || m.from.length > best.from.length)) best = m;
  }
  return best === null ? path : best.to + path.slice(best.from.length);
}

/** The set after `moves`: every path the moves reach is replaced by its new place. */
export function mapSet(paths: Iterable<string>, moves: readonly Move[]): Set<string> {
  return new Set([...paths].map((p) => mapPath(p, moves)));
}

/** The moves that undo `moves`. */
export function reversed(moves: readonly Move[]): Move[] {
  return moves.map((m) => ({ from: m.to, to: m.from })).reverse();
}

// ---- names -----------------------------------------------------------------------------------------

export type EntryProblem =
  "empty" | "invalidChars" | "invalidEnd" | "reserved" | "tooLong" | "exists";

export const ENTRY_PROBLEM_TEXT: Record<EntryProblem, MessageKey> = {
  empty: "fileOps.nameEmpty",
  invalidChars: "fileOps.nameInvalidChars",
  invalidEnd: "fileOps.nameInvalidEnd",
  reserved: "fileOps.nameReserved",
  tooLong: "fileOps.nameTooLong",
  exists: "fileOps.nameExists",
};

const FORBIDDEN = /[\\/:*?"<>|\u0000-\u001f]/;
const RESERVED = new Set([
  "con",
  "prn",
  "aux",
  "nul",
  ...Array.from({ length: 9 }, (_, i) => `com${i + 1}`),
  ...Array.from({ length: 9 }, (_, i) => `lpt${i + 1}`),
]);

/**
 * What is wrong with a new name for a file or folder, or null. `siblings` are the names in the folder (compared
 * without regard to case); `current` is the entry's own name when it is being renamed, which does not clash with
 * itself, so a change of case alone is fine.
 */
export function entryNameProblem(
  name: string,
  siblings: readonly string[],
  current: string | null = null,
): EntryProblem | null {
  if (name.trim() === "") return "empty";
  if (FORBIDDEN.test(name)) return "invalidChars";
  if (/[. ]$/.test(name) || name.startsWith(" ")) return "invalidEnd";
  if (RESERVED.has(name.split(".")[0].trimEnd().toLowerCase())) return "reserved";
  if (new TextEncoder().encode(name).length > 255) return "tooLong";
  const lower = name.toLowerCase();
  const others = siblings.filter((s) => s !== current);
  if (others.some((s) => s.toLowerCase() === lower)) return "exists";
  return null;
}

/** The first of "New folder", "New folder 2"... that is not taken in the folder (names compare without regard to case). */
export function freshFolderName(base: string, siblings: readonly string[]): string {
  const taken = new Set(siblings.map((s) => s.toLowerCase()));
  if (!taken.has(base.toLowerCase())) return base;
  for (let n = 2; ; n++) {
    const name = `${base} ${n}`;
    if (!taken.has(name.toLowerCase())) return name;
  }
}

/** Where the part of a name that is typed over is: the stem of a file, the whole name of a folder. */
export function renameSelection(name: string, isDir: boolean): [number, number] {
  const dot = name.lastIndexOf(".");
  return [0, !isDir && dot > 0 ? dot : name.length];
}

// ---- selection -------------------------------------------------------------------------------------

export type ClickModifiers = { ctrl: boolean; shift: boolean };

/** The paths between `a` and `b` in the order of `rows`, both included (just `b` when `a` is not a row). */
export function rangeBetween(rows: readonly string[], a: string | null, b: string): string[] {
  const to = rows.indexOf(b);
  const from = a === null ? -1 : rows.indexOf(a);
  if (to < 0) return [];
  if (from < 0) return [b];
  return rows.slice(Math.min(from, to), Math.max(from, to) + 1);
}

/**
 * The selection after a click on `clicked`. Ctrl toggles it, Shift selects from the anchor (the row clicked
 * last without Shift), both together add that range, and a plain click selects only the row. `plain` says
 * nothing was extended, so the click may also open the row.
 */
export function selectAfterClick(
  rows: readonly string[],
  selection: ReadonlySet<string>,
  anchor: string | null,
  clicked: string,
  mods: ClickModifiers,
): { selection: Set<string>; anchor: string; plain: boolean } {
  if (mods.shift) {
    const range = rangeBetween(rows, anchor ?? clicked, clicked);
    const next = mods.ctrl ? new Set([...selection, ...range]) : new Set(range);
    return { selection: next, anchor: anchor ?? clicked, plain: false };
  }
  if (mods.ctrl) {
    const next = new Set(selection);
    if (next.has(clicked)) next.delete(clicked);
    else next.add(clicked);
    return { selection: next, anchor: clicked, plain: false };
  }
  return { selection: new Set([clicked]), anchor: clicked, plain: true };
}

/** The selection for a key press that extends it (Shift+Arrow): the range from the anchor to the new row. */
export function extendSelection(
  rows: readonly string[],
  anchor: string | null,
  focused: string,
): Set<string> {
  return new Set(rangeBetween(rows, anchor ?? focused, focused));
}

/**
 * What a context menu or a key acts on: the whole selection when the row it was used on is part of it, else just
 * that row.
 */
export function targetsOf(selection: ReadonlySet<string>, row: string): string[] {
  return selection.has(row) && selection.size > 1 ? [...selection] : [row];
}

// ---- undo ------------------------------------------------------------------------------------------

export type UndoEntry = {
  /** The id the backend gave the operation. */
  opId: number;
  /** What it did, in a sentence ("Moved 2 files"), for the feed row of the undo. */
  label: string;
  /** The moves that were made, for the UI to follow them back. */
  moves: Move[];
  /** The paths of folders that were created. */
  folders: string[];
};

/** The operations that can still be undone, newest last. */
export class UndoStack {
  private entries: UndoEntry[] = [];

  constructor(private readonly limit = 20) {}

  get size(): number {
    return this.entries.length;
  }

  push(entry: UndoEntry): void {
    this.entries.push(entry);
    if (this.entries.length > this.limit) this.entries.shift();
  }

  peek(): UndoEntry | null {
    return this.entries[this.entries.length - 1] ?? null;
  }

  pop(): UndoEntry | null {
    return this.entries.pop() ?? null;
  }

  /** Drops one operation (the toast's Undo was used on it). */
  remove(opId: number): UndoEntry | null {
    const at = this.entries.findIndex((e) => e.opId === opId);
    return at < 0 ? null : (this.entries.splice(at, 1)[0] ?? null);
  }

  clear(): void {
    this.entries = [];
  }
}

// ---- what happened, for the toast and the feed -----------------------------------------------------

/** A move that renames: one item, same folder. */
export function isRename(moves: readonly Move[]): boolean {
  return moves.length === 1 && dirname(moves[0].from) === dirname(moves[0].to);
}

/** "2 files", "1 folder" or "3 items" (when files and folders are mixed). `dirs` says which of the items are folders. */
export function itemsPhrase(dirs: readonly boolean[]): string {
  if (dirs.every((isDir) => !isDir)) return tn("fileOps.files", dirs.length);
  if (dirs.every(Boolean)) return tn("fileOps.folders", dirs.length);
  return tn("fileOps.items", dirs.length);
}

/** The sentence for a move: "Renamed a.md to b.md", "Moved 2 files, 7 links updated". */
export function moveSummary(
  moves: readonly Move[],
  dirs: readonly boolean[],
  links: number,
): string {
  const base = isRename(moves)
    ? t("fileOps.summaryRenamed", { from: basename(moves[0].from), to: basename(moves[0].to) })
    : t("fileOps.summaryMoved", { what: itemsPhrase(dirs) });
  return links > 0
    ? t("fileOps.withLinks", { summary: base, links: tn("fileOps.linksUpdated", links) })
    : base;
}

/** The sentence for a delete: "Deleted 3 items". */
export function deleteSummary(dirs: readonly boolean[]): string {
  return t("fileOps.summaryDeleted", { what: itemsPhrase(dirs) });
}

/** One line for each document that was left alone: "guide.md: changed in the meantime". */
export function skipNotes(skipped: readonly SkippedFile[]): string[] {
  return skipped.map((s) => `${basename(s.path)}: ${t(`fileOps.reason.${s.reason}`)}`);
}
