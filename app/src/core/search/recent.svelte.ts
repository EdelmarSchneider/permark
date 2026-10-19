// The files this window opened in the project, newest first: Quick open lists them before anything is typed.
// Kept in the project session (`search.recent`), like the rest of the person's place.

import type { Api } from "../../api";

const KEY = "search.recent";
/** Remembered files. */
export const MAX_RECENT = 30;

/** Reads the stored list: only strings, no repeats, at most [`MAX_RECENT`]. */
export function parseRecent(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  for (const item of value) {
    if (typeof item === "string" && item !== "") seen.add(item);
    if (seen.size === MAX_RECENT) break;
  }
  return [...seen];
}

/** `path` first, the rest after it in order. */
export function bumped(list: readonly string[], path: string): string[] {
  return [path, ...list.filter((p) => p !== path)].slice(0, MAX_RECENT);
}

class RecentFiles {
  list = $state.raw<readonly string[]>([]);
  #api: Api | null = null;

  /** A project's list is read; what was opened before it arrived stays first. */
  async load(api: Api): Promise<void> {
    this.#api = api;
    this.list = [];
    const stored = await api.projectSessionGet(KEY);
    if (api !== this.#api || !stored.ok) return;
    const before = this.list;
    this.list = [...before, ...parseRecent(stored.value).filter((p) => !before.includes(p))].slice(
      0,
      MAX_RECENT,
    );
  }

  /** A document of the project was opened. */
  visit(path: string): void {
    this.list = bumped(this.list, path);
    void this.#api?.projectSessionSet(KEY, [...this.list]);
  }
}

export const recentFiles = new RecentFiles();
