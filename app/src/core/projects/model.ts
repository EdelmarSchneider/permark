// Projects inside the opened folder, as the UI derives them from `ProjectsView` (`projects.md` §12): which strip shows,
// the rows Settings groups, the project that owns a path, a project's hue. Pure; the store and the views call these.

import type { InnerLine, MoveAsk, ProjectRow, ProjectsView } from "../../api";
import { hueOf, type Hue } from "../shell/switcherModel";

/** What the Explorer strip asks (one at a time; `none`: nothing to ask, or not yet). */
export type Strip =
  | { kind: "none" }
  /** First question for this folder (nothing answered yet). */
  | { kind: "first"; count: number; names: readonly string[]; inner: readonly InnerLine[] }
  /** Projects found after the folder was answered: where they are (`paths`: offered ones, path order). */
  | {
      kind: "new";
      count: number;
      names: readonly string[];
      paths: readonly string[];
      inner: readonly InnerLine[];
    }
  /** Is this found project the missing one? */
  | { kind: "move"; ask: MoveAsk }
  /** The project list can't be read: the Explorer says so (Settings shows the problem). */
  | { kind: "broken" };

/** The project list can't be written now (unreadable | written by a newer PerMark). */
export function readOnly(view: ProjectsView): boolean {
  return view.record.status === "unreadable" || view.record.status === "newer";
}

/** Where an answer to the offer is saved, said before the person answers: a list that exists is updated, else
 *  PerMark creates it (S2-1). */
export function recordPlace(
  view: ProjectsView | null,
): "projects.record.saves" | "projects.record.creates" {
  return view === null || view.record.status === "absent"
    ? "projects.record.creates"
    : "projects.record.saves";
}

/** The strip for a view. An unreadable list says so (a pull | an editor broke it: nothing else would show it). Else
 *  nothing while the walk runs, in a plain folder, or while the list is newer than this PerMark (Settings says why).
 *  The move question comes first, then the first question | new projects. */
export function stripOf(view: ProjectsView | null): Strip {
  if (view === null) return { kind: "none" };
  if (view.record.status === "unreadable") return { kind: "broken" };
  if (!view.present || !view.ready || readOnly(view)) return { kind: "none" };
  if (view.ask !== null) return { kind: "move", ask: view.ask };
  const { count, names, inner } = view.offered;
  if (count === 0) return { kind: "none" };
  switch (view.answer) {
    case "unanswered":
      return { kind: "first", count, names, inner };
    case "answered":
      return { kind: "new", count, names, paths: offeredPaths(view), inner };
    case "declinedAll":
      return { kind: "none" };
    default:
      return view.answer satisfies never;
  }
}

/** Identity of a path for comparison: `/` separators, case folded (the backend's keys fold case where the system
 *  does; a tree path and a row path of one folder always match this way). */
export function keyOf(path: string): string {
  return path.replaceAll("\\", "/").normalize("NFC").toLowerCase();
}

/** Included projects by key of their path. */
export function membersOf(view: ProjectsView | null): ReadonlyMap<string, ProjectRow> {
  const map = new Map<string, ProjectRow>();
  if (view === null) return map;
  for (const row of view.rows) if (row.state === "included") map.set(keyOf(row.path), row);
  return map;
}

/** The included project that owns a path (deepest one containing it, the folder itself included); `null`: the opened
 *  folder owns it. Worktree paths (`@wt/...`) belong to the opened folder. */
export function ownerOf(
  members: ReadonlyMap<string, ProjectRow>,
  path: string | null,
): ProjectRow | null {
  if (path === null || members.size === 0 || path.startsWith("@wt/")) return null;
  let key = keyOf(path);
  for (;;) {
    const found = members.get(key);
    if (found) return found;
    const cut = key.lastIndexOf("/");
    if (cut <= 0) return null;
    key = key.slice(0, cut);
  }
}

/** A path relative to a project (`""`: the project folder itself). */
export function relativeTo(row: ProjectRow, path: string): string {
  const rel = path.replaceAll("\\", "/").slice(row.path.length);
  return rel.startsWith("/") ? rel.slice(1) : rel;
}

/** The hue of a project: the one its tab chip wears when opened alone (same hash of its full path). */
export function hueOfMember(root: string, row: Pick<ProjectRow, "path">): Hue {
  const base = root.replaceAll("\\", "/").replace(/\/+$/, "");
  return hueOf({ kind: "project", path: `${base}/${row.path}` });
}

/** Settings > Projects groups. */
export type Groups = {
  included: readonly ProjectRow[];
  notIncluded: readonly ProjectRow[];
  notFound: readonly ProjectRow[];
};

/** A row matches the filter by name, short name or path (case-insensitive; empty filter: every row). */
export function matches(row: ProjectRow, filter: string): boolean {
  const words = filter.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const hay = `${row.name}\n${row.short}\n${row.path}`.toLowerCase();
  return words.every((w) => hay.includes(w));
}

/** Rows by group, in path order (nesting by indent reads right), filtered. */
export function groupsOf(rows: readonly ProjectRow[], filter: string): Groups {
  const shown = rows
    .filter((r) => matches(r, filter))
    .sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
  return {
    included: shown.filter((r) => r.state === "included"),
    notIncluded: shown.filter((r) => r.state === "offered" || r.state === "declined"),
    notFound: shown.filter((r) => r.state === "missing"),
  };
}

/** Indent of a row: how many rows of the same group contain it (`parent` chain; a broken chain ends the count). */
export function depthOf(row: ProjectRow, byKey: ReadonlyMap<string, ProjectRow>): number {
  let depth = 0;
  let at = row.parent;
  const seen = new Set<string>();
  while (at !== null && !seen.has(at)) {
    seen.add(at);
    const up = byKey.get(at);
    if (!up) break;
    if (up.state === row.state || (isListed(up) && isListed(row))) depth += 1;
    at = up.parent;
  }
  return Math.min(depth, 6);
}

function isListed(row: ProjectRow): boolean {
  return row.state === "offered" || row.state === "declined";
}

/** A long path cut in the middle: its start and its last folder stay (`…` between); short paths unchanged. */
export function middleCut(path: string, max = 48): string {
  const chars = Array.from(path);
  if (chars.length <= max) return path;
  const parts = path.split("/");
  const last = parts.at(-1) ?? path;
  const tailChars = Array.from(last);
  if (tailChars.length >= max - 4) return `…${tailChars.slice(-(max - 1)).join("")}`;
  const headRoom = max - tailChars.length - 2;
  return `${chars.slice(0, headRoom).join("")}…/${last}`;
}

/** Names joined for the strip ("a, b and c"), in the UI language. */
export function joinNames(names: readonly string[], locale: string): string {
  try {
    return new Intl.ListFormat(locale, { style: "long", type: "conjunction" }).format(names);
  } catch {
    return names.join(", ");
  }
}

/** Offered rows the Choose screen lists, pre-checked (inner roots' projects grouped under them by path order). */
export function offeredPaths(view: ProjectsView | null): string[] {
  if (view === null) return [];
  return view.rows
    .filter((r) => r.state === "offered")
    .sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0))
    .map((r) => r.path);
}

/** Where new projects are, as the strip says it: one path | up to 3 paths | a count in the folder holding most of them
 *  and how many other folders (`folder` `""`: the opened folder itself). */
export type NewWhere =
  | { kind: "one"; path: string }
  | { kind: "few"; paths: readonly string[] }
  | { kind: "many"; count: number; folder: string; others: number };

export function newWhere(paths: readonly string[], count: number): NewWhere {
  if (count === 1 && paths.length === 1) return { kind: "one", path: paths[0] ?? "" };
  if (count <= 3 && paths.length === count) return { kind: "few", paths };
  const folders = new Map<string, number>();
  for (const path of paths) {
    const cut = path.lastIndexOf("/");
    const folder = cut < 0 ? "" : path.slice(0, cut);
    folders.set(folder, (folders.get(folder) ?? 0) + 1);
  }
  let folder = "";
  let most = -1;
  for (const [name, n] of folders) {
    // most projects first; a tie goes to the first in path order (the map keeps it)
    if (n > most) {
      folder = name;
      most = n;
    }
  }
  return { kind: "many", count, folder, others: Math.max(0, folders.size - 1) };
}

/** The folder name of a path (its last segment). */
export function folderName(path: string): string {
  const parts = path.replaceAll("\\", "/").split("/").filter(Boolean);
  return parts.at(-1) ?? path;
}

/** The short name a project node shows after its folder name: only when it says something else. */
export function aliasOf(row: Pick<ProjectRow, "path" | "short">): string | null {
  const folder = folderName(row.path);
  const short = row.short.trim();
  return short === "" || keyOf(short) === keyOf(folder) ? null : short;
}
