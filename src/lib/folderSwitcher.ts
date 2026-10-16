// The folder switcher: the menu on the project's name in the Explorer. It lists the recent folders, marks the one this window
// shows, and offers to open another folder, close this one, or forget an entry. Pure: the rows come from the lists the app keeps.

import type { RecentVault } from "./types";

export type SwitcherRow = {
  root: string;
  name: string;
  /** The folder this window shows. */
  current: boolean;
  /** It failed to open in this run: shown dimmed, and only "Remove" works on it. */
  missing: boolean;
  /** The folder that holds it, shortened (see `parentLabel`), to tell apart two folders with the same name. */
  parent: string;
};

/** A folder path as lists compare it: "/" separators, no trailing separator, and no difference of case on Windows paths. */
export function folderKey(path: string): string {
  let key = path.replace(/\\/g, "/");
  // a drive root ("C:/") and the root of the file system ("/") keep their separator
  if (key.length > 1 && !/^[A-Za-z]:\/$/.test(key)) key = key.replace(/\/+$/, "");
  return /^([A-Za-z]:|\/\/)/.test(key) ? key.toLowerCase() : key;
}

/** Whether two paths name the same folder. */
export function sameFolder(a: string | null | undefined, b: string | null | undefined): boolean {
  return typeof a === "string" && typeof b === "string" && folderKey(a) === folderKey(b);
}

/** The folder that holds `path`, written with the separators `path` has; "" when it has none. */
export function parentOf(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const at = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  if (at < 0) return "";
  const parent = trimmed.slice(0, at);
  // "F:\work" is held by "F:\", and "/home" by "/"
  return parent === "" || /^[A-Za-z]:$/.test(parent) ? trimmed.slice(0, at + 1) : parent;
}

/**
 * The folder that holds `path`, cut to its own name when it is deep ("…\team-b"), so the part that tells two folders of the same
 * name apart stays in sight in a narrow menu. The whole path is the row's tooltip.
 */
export function parentLabel(path: string): string {
  const parent = parentOf(path);
  const names = parent.split(/[\\/]/).filter(Boolean);
  if (names.length <= 2) return parent;
  const separator = parent.includes("\\") ? "\\" : "/";
  return `…${separator}${names[names.length - 1]}`;
}

type RowsInput = {
  /** The recent folders, newest first. */
  recents: readonly RecentVault[];
  /** The folders the person removed from the list. */
  hidden: ReadonlySet<string>;
  /** The folders that failed to open in this run. */
  missing: ReadonlySet<string>;
  /** The folder this window shows, and the name it has. */
  current: { root: string; name: string } | null;
};

/**
 * What the menu lists: the folder this window shows first (always, even when it was removed from the list or is not in it yet),
 * then the other recent folders in the order the list keeps them, without the ones the person removed.
 */
export function switcherRows({ recents, hidden, missing, current }: RowsInput): SwitcherRow[] {
  const hiddenKeys = new Set([...hidden].map(folderKey));
  const missingKeys = new Set([...missing].map(folderKey));
  const rows: SwitcherRow[] = [];
  const seen = new Set<string>();
  const add = (root: string, name: string, isCurrent: boolean): void => {
    const key = folderKey(root);
    if (seen.has(key)) return;
    seen.add(key);
    rows.push({
      root,
      name,
      current: isCurrent,
      missing: !isCurrent && missingKeys.has(key),
      parent: parentLabel(root),
    });
  };
  if (current) add(current.root, current.name, true);
  for (const recent of recents) {
    if (!hiddenKeys.has(folderKey(recent.root))) add(recent.root, recent.name, false);
  }
  return rows;
}

/** The rows the menu can switch to: every one but the current folder and the ones that are not found. */
export function switchable(rows: readonly SwitcherRow[]): SwitcherRow[] {
  return rows.filter((row) => !row.current && !row.missing);
}
