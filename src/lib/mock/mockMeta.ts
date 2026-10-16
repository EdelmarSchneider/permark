// Tags and descriptions for the mock backend: app tags kept in memory, file tags read from the mock
// documents' front matter. A small port of the Rust catalog, enough to drive the UI in a browser.

import { emitMock } from "../api";
import { hasLook, withStyle, type TagColor, type TagIcon, type TagStyles } from "../tagStyle";
import { normalizeDescription, normalizeTag, normalizeTags } from "../tagText";
import type {
  FileMeta,
  IndexEntry,
  MetaProblem,
  TagCount,
  TaggedFile,
  TagOverview,
  TagRef,
} from "../types";
import { allDocPaths, docExists, MOCK_NOW, mockFrontmatter, mockState } from "./fixtures";

type Entry = { tags: string[]; description: string };

const entries = new Map<string, Entry>([
  [
    "docs/specs/live-feed.md",
    {
      tags: ["spec", "needs-review"],
      description: "How the live feed groups watcher events into rows.",
    },
  ],
  [
    "docs/specs/watcher.md",
    { tags: ["spec"], description: "Debounce and reconcile: from raw events to batches." },
  ],
  [
    "docs/research/storage.md",
    { tags: ["research", "needs-review"], description: "Where state lives, and why." },
  ],
  ["docs/world/lore.md", { tags: ["worldbuilding"], description: "" }],
  ["plan.md", { tags: [], description: "The work in three tasks, and where we are." }],
  // the file is gone: it shows up as a missing-file entry
  ["docs/old/retired.md", { tags: ["spec"], description: "Replaced by the live feed spec." }],
]);

/** The looks of tags, by name: a couple are set so the mock shows chips in color. */
let styles: TagStyles = new Map([
  ["spec", { color: "blue", icon: "book-open" }],
  ["needs-review", { color: "orange", icon: "flag" }],
]);

/** What the mock controls toggle: an unreadable meta.toml (a merge conflict). */
export const mockMeta = { problem: null as MetaProblem | null };

const CONFLICT: MetaProblem = {
  message: "invalid TOML: expected a key, found `<<<<<<<`",
  line: 12,
};

export function setMetaProblem(on: boolean): void {
  mockMeta.problem = on ? CONFLICT : null;
  notify();
}

function notify(): void {
  setTimeout(() => emitMock("meta-changed", null), 0);
}

const inWorktree = (path: string): boolean => path.startsWith("@wt/");

function fileTagsOf(path: string): string[] {
  const pair = mockFrontmatter(path)?.find(([key]) => key.toLowerCase() === "tags");
  return pair ? normalizeTags(pair[1].split(",")) : [];
}

function combine(file: string[], app: string[]): TagRef[] {
  return [
    ...file.map((name) => ({ name, file: true, app: app.includes(name) })),
    ...app.filter((name) => !file.includes(name)).map((name) => ({ name, file: false, app: true })),
  ];
}

function guard(path: string): void {
  if (inWorktree(path)) {
    throw { kind: "metaWorktree", message: "tags are not stored for files of linked worktrees" };
  }
  if (mockMeta.problem) {
    throw {
      kind: "metaUnreadable",
      message: `meta.toml can't be read (line ${mockMeta.problem.line}): ${mockMeta.problem.message}`,
    };
  }
}

export function fileMeta(path: string): FileMeta {
  if (!docExists(path)) throw { kind: "notFound", message: `not found: ${path}` };
  const entry = inWorktree(path) ? undefined : entries.get(path);
  return {
    path,
    tags: combine(fileTagsOf(path), entry?.tags ?? []),
    description: entry?.description ?? "",
    writable: !inWorktree(path) && mockMeta.problem === null,
  };
}

function store(path: string, change: (entry: Entry) => void): FileMeta {
  guard(path);
  const entry = { ...(entries.get(path) ?? { tags: [], description: "" }) };
  change(entry);
  if (entry.tags.length === 0 && entry.description === "") entries.delete(path);
  else entries.set(path, entry);
  notify();
  return fileMeta(path);
}

export const setTags = (path: string, tags: string[]): FileMeta =>
  store(path, (e) => {
    e.tags = normalizeTags(tags);
  });

export const setDescription = (path: string, description: string): FileMeta =>
  store(path, (e) => {
    e.description = normalizeDescription(description);
  });

type Row = { path: string; app: string[]; file: string[]; description: string };

function rows(): Row[] {
  return allDocPaths()
    .filter((path) => !inWorktree(path))
    .map((path) => ({
      path,
      app: entries.get(path)?.tags ?? [],
      file: fileTagsOf(path),
      description: entries.get(path)?.description ?? "",
    }))
    .sort((a, b) => a.path.localeCompare(b.path));
}

const tagsOf = (row: Row): string[] => [...new Set([...row.file, ...row.app])];

function missingPaths(): string[] {
  return [...entries.keys()].filter((path) => !docExists(path));
}

export function listTags(): TagOverview {
  const all = rows();
  const counts = new Map<string, TagCount>();
  for (const row of all) {
    for (const name of tagsOf(row)) {
      const count = counts.get(name) ?? { name, count: 0, onlyInFiles: true };
      count.count += 1;
      count.onlyInFiles &&= !row.app.includes(name);
      counts.set(name, count);
    }
  }
  return {
    tags: [...counts.values()].sort((a, b) => b.count - a.count || a.name.localeCompare(b.name)),
    untagged: all.filter((row) => tagsOf(row).length === 0).length,
    missing: missingPaths().length,
    status: { problem: mockMeta.problem, writeError: null, pendingChanges: 0 },
  };
}

export function filesWithTags(tags: string[], untagged: boolean): TaggedFile[] {
  const wanted = normalizeTags(tags);
  if (!untagged && wanted.length === 0) return [];
  return rows()
    .filter((row) =>
      untagged ? tagsOf(row).length === 0 : wanted.every((tag) => tagsOf(row).includes(tag)),
    )
    .map((row) => ({
      path: row.path,
      description: row.description,
      modifiedMs: mockState.modified.get(row.path) ?? MOCK_NOW,
    }));
}

export function metaIndex(): IndexEntry[] {
  return rows()
    .filter((row) => tagsOf(row).length > 0 || row.description !== "")
    .map((row) => ({
      path: row.path,
      tags: tagsOf(row).sort(),
      description: row.description,
      modifiedMs: mockState.modified.get(row.path) ?? MOCK_NOW,
    }));
}

export function cleanUp(): number {
  guard("");
  const missing = missingPaths();
  for (const path of missing) entries.delete(path);
  if (missing.length > 0) notify();
  return missing.length;
}

export function tagStyles(): TagStyles {
  return styles;
}

export function setTagStyle(tag: string, color: TagColor | null, icon: TagIcon | null): TagStyles {
  guard("");
  const name = normalizeTag(tag);
  if (name === null) return styles;
  const next = { color, icon };
  styles = withStyle(styles, name, hasLook(next) ? next : null);
  notify();
  return styles;
}
