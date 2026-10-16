// Search everywhere (Ctrl+T): the scopes of the box, the prefixes that switch them, the rows each scope lists, how the
// All scope groups them, and the keyboard order. Pure; `SearchEverywhere.svelte` feeds it the state of the stores and
// draws what it returns. File matching is Quick open's (`quickOpenModel.ts`); sections come from the search index.

import type { CommandAction, CommandMatch } from "./commands";
import { basename, folderLabel, splitWorktree } from "./paths";
import type { FoundFolder, QuickMatches } from "./quickOpenModel";
import { sanitizeSnippet } from "./searchModel";
import { rankTags, type NamedCount } from "./tagModel";
import { hasAllTags } from "./tagQuery";
import type { SearchHit } from "./types";

// ---- scopes and prefixes ------------------------------------------------------------------------------

export type Scope = "all" | "files" | "sections" | "tags" | "commands";

/** The scopes in the order of the tab strip, and the order Tab walks them. */
export const SCOPES: readonly Scope[] = ["all", "files", "sections", "tags", "commands"];

/** Simple mode has no folder to search: only the commands of the app apply. */
const COMMANDS_ONLY: readonly Scope[] = ["commands"];

/** The scopes the box offers: all of them with a folder open, else the commands. */
export function scopesFor(ide: boolean): readonly Scope[] {
  return ide ? SCOPES : COMMANDS_ONLY;
}

/** The scope itself when the box offers it, else the first one it does. */
export function allowedScope(scopes: readonly Scope[], scope: Scope): Scope {
  return scopes.includes(scope) ? scope : scopes[0];
}

/** The scope after (1) or before (-1) the current one, wrapping around the ends. */
export function stepScope(scopes: readonly Scope[], current: Scope, delta: 1 | -1): Scope {
  const at = Math.max(0, scopes.indexOf(current));
  return scopes[(at + delta + scopes.length) % scopes.length];
}

/**
 * A prefix typed as the first character of the box switches the scope and leaves the text: `>` is Commands, `#` is
 * Sections. A prefix whose scope the box does not offer is ordinary text.
 */
export function applyPrefix(
  raw: string,
  scope: Scope,
  scopes: readonly Scope[],
): { scope: Scope; text: string } {
  const target: Scope | null = raw.startsWith(">")
    ? "commands"
    : raw.startsWith("#")
      ? "sections"
      : null;
  if (target === null || !scopes.includes(target)) return { scope, text: raw };
  return { scope: target, text: raw.slice(1).replace(/^\s+/, "") };
}

// ---- rows ---------------------------------------------------------------------------------------------

export type FileRow = {
  type: "file";
  key: string;
  path: string;
  /** Character positions of the match inside the full path. */
  positions: number[];
  /** Character positions of the match inside the description (when that is why the file is listed). */
  descriptionPositions: number[];
  description: string;
  /** When the file last changed (the "recently changed" rows and any file in the change feed); null otherwise. */
  ms: number | null;
  group?: "changed" | "opened";
  /** A file the Explorer filter hides, listed because the hidden matches were revealed. */
  hidden?: boolean;
};

/** A folder whose path has the words typed. Enter reveals it in the Explorer. */
export type FolderRow = {
  type: "folder";
  key: string;
  path: string;
  /** Character positions of the match inside the full path. */
  positions: number[];
};

export type SectionRow = {
  type: "section";
  key: string;
  path: string;
  /** The section's own heading (the file name for text above the first heading). */
  title: string;
  /** File name, then the headings above the section: "auth-flow.md › Login". */
  where: string;
  /** A snippet with `<mark>` around the matched words (sanitized); null for an entry of the outline. */
  snippet: string | null;
  /** The search hit (open at its line, finding the query), or null for an entry of the outline. */
  hit: SearchHit | null;
  /** The heading's index in the open document's outline, or null for a search hit. */
  outline: number | null;
};

export type TagRow = {
  type: "tag";
  key: string;
  name: string;
  count: number;
  /** Positions of the matched characters of the name. */
  positions: number[];
};

export type CommandRow = {
  type: "command";
  key: string;
  id: CommandAction;
  label: string;
  positions: number[];
  /** The English label, when that is what matched and the label shown is in another language. */
  alias: string | null;
  /** The key that runs it, written for the platform; null when it has none. */
  shortcut: string | null;
};

/** The "+N hidden files match" row of the Files scope. */
export type MoreRow = { type: "more"; key: "more"; count: number };

export type Row = FileRow | FolderRow | SectionRow | TagRow | CommandRow | MoreRow;

/** How many recent files of each kind an empty box lists. */
export const RECENT_LIMIT = 8;
/** The most file or section rows a scope lists. */
export const RESULT_LIMIT = 50;
/** The most rows each group of the All scope lists; the rest is behind "Show all". */
export const GROUP_LIMIT = { files: 5, folders: 3, sections: 5, tags: 3, commands: 3 } as const;

export type RecentInput = {
  /** Files that changed, newest first (documents only). */
  changed: readonly { path: string; atMs: number }[];
  /** Files opened, most recent first. */
  opened: readonly string[];
  /** The files the box may list (the Explorer filter's). */
  exists: ReadonlySet<string>;
  describe: (path: string) => string;
};

/** What the box lists before anything is typed: the files that changed, then the ones opened, as Quick open does. */
export function recentFileRows(input: RecentInput): FileRow[] {
  const { changed, opened, exists, describe } = input;
  const rows: FileRow[] = [];
  for (const entry of changed) {
    if (rows.length >= RECENT_LIMIT) break;
    if (!exists.has(entry.path) || rows.some((r) => r.path === entry.path)) continue;
    rows.push({
      type: "file",
      key: `file:${entry.path}`,
      path: entry.path,
      positions: [],
      descriptionPositions: [],
      description: describe(entry.path),
      ms: entry.atMs,
      group: "changed",
    });
  }
  const seen = new Set(rows.map((r) => r.path));
  let openedRows = 0;
  for (const path of opened) {
    if (openedRows >= RECENT_LIMIT) break;
    if (!exists.has(path) || seen.has(path)) continue;
    openedRows++;
    rows.push({
      type: "file",
      key: `file:${path}`,
      path,
      positions: [],
      descriptionPositions: [],
      description: describe(path),
      ms: null,
      group: "opened",
    });
  }
  return rows;
}

/** The rows for the files a query found (Quick open's matches). */
export function fileRows(
  found: QuickMatches["found"],
  describe: (path: string) => string,
  changedAt: ReadonlyMap<string, number>,
): FileRow[] {
  return found.map((f) => ({
    type: "file",
    key: `file:${f.path}`,
    path: f.path,
    positions: f.positions,
    descriptionPositions: f.descriptionPositions,
    description: describe(f.path),
    ms: changedAt.get(f.path) ?? null,
    hidden: f.hidden || undefined,
  }));
}

/** The rows for the folders a query found. */
export function folderRows(found: readonly FoundFolder[]): FolderRow[] {
  return found.map((f) => ({
    type: "folder",
    key: `folder:${f.path}`,
    path: f.path,
    positions: f.positions,
  }));
}

const SEPARATOR = " › ";

/** "auth-flow.md", or "auth-flow.md @wt feature" for a copy in a linked worktree. */
function fileLabel(path: string): string {
  const { worktree } = splitWorktree(path);
  return worktree === null ? basename(path) : `${basename(path)} @wt ${worktree}`;
}

function sectionOfHit(hit: SearchHit): SectionRow {
  const trail = hit.headingPath;
  const aboveHeadings = trail.length === 0;
  return {
    type: "section",
    key: `section:${hit.path}:${hit.startLine}`,
    path: hit.path,
    title: aboveHeadings ? basename(hit.path) : trail[trail.length - 1],
    where: aboveHeadings
      ? folderLabel(hit.path)
      : [fileLabel(hit.path), ...trail.slice(0, -1)].join(SEPARATOR),
    snippet: sanitizeSnippet(hit.snippet),
    hit,
    outline: null,
  };
}

export type SectionsInput = {
  /** What the search index answered, best first. */
  hits: readonly SearchHit[];
  /** The files the Explorer filter keeps: a hit in any other file is left out. */
  visible: ReadonlySet<string>;
  /** Hits must be in files that have all of these tags. */
  tags: readonly string[];
  tagsOf: (path: string) => readonly string[] | undefined;
  limit: number;
};

/**
 * The sections a search found: in the files the Explorer shows and (with `tag:` filters) with the tags, best first,
 * those of the project itself before the copies in linked worktrees.
 */
export function sectionRows(input: SectionsInput): SectionRow[] {
  const { hits, visible, tags, tagsOf, limit } = input;
  const kept = hits.filter((h) => visible.has(h.path) && hasAllTags(tagsOf(h.path), tags));
  const own = kept.filter((h) => splitWorktree(h.path).worktree === null);
  const copies = kept.filter((h) => splitWorktree(h.path).worktree !== null);
  return [...own, ...copies].slice(0, limit).map(sectionOfHit);
}

/** The headings of the outline of the open document, as far as the rows need them. */
export type OutlineEntry = { index: number; text: string; parent: number | null };

/** The sections of the open document, from its outline: what the Sections scope lists before anything is typed. */
export function outlineRows(
  path: string,
  items: readonly OutlineEntry[],
  untitled: string,
): SectionRow[] {
  const label = (item: OutlineEntry): string => item.text || untitled;
  return items.map((item) => {
    const above: string[] = [];
    for (let at = item.parent; at !== null; at = items[at]?.parent ?? null) {
      above.unshift(label(items[at]));
    }
    return {
      type: "section",
      key: `outline:${item.index}`,
      path,
      title: label(item),
      where: [fileLabel(path), ...above].join(SEPARATOR),
      snippet: null,
      hit: null,
      outline: item.index,
    };
  });
}

/** The tags whose name matches the words, tags on more files first. Tags already chosen as filters are left out. */
export function tagRows(
  words: string,
  counts: readonly NamedCount[],
  chosen: readonly string[],
): TagRow[] {
  return rankTags(words, counts, new Set(chosen)).map((m) => ({
    type: "tag",
    key: `tag:${m.tag.name}`,
    name: m.tag.name,
    count: m.tag.count,
    positions: m.positions,
  }));
}

/** The rows for the commands a query found; `shortcut` writes a key for the platform and the language. */
export function commandRows(
  matches: readonly CommandMatch[],
  shortcut: (combo: string) => string,
): CommandRow[] {
  return matches.map((m) => ({
    type: "command",
    key: `command:${m.command.id}`,
    id: m.command.id,
    label: m.label,
    positions: m.positions,
    alias: m.alias,
    shortcut: m.command.shortcut === undefined ? null : shortcut(m.command.shortcut),
  }));
}

// ---- the view -----------------------------------------------------------------------------------------

/** What a group is called: a scope, or one of the two lists of recent files. */
export type GroupTitle = "changed" | "opened" | "folders" | Exclude<Scope, "all">;

export type Group = {
  id: string;
  /** The header; null for a group that has none (a single scope lists its rows under no header). */
  title: GroupTitle | null;
  rows: Row[];
  /** How many results the scope has. */
  total: number;
  /** The scope "Show all (N)" switches to; null when every result is listed already. */
  showAll: Exclude<Scope, "all"> | null;
};

export type View = {
  groups: Group[];
  /** Every row in the order of the keyboard: the groups top to bottom. Headers are not rows. */
  rows: Row[];
};

export type ViewInput = {
  scope: Scope;
  /** Neither words nor `tag:` filters are typed. */
  idle: boolean;
  /** Words are typed (besides any `tag:` filters). */
  hasWords: boolean;
  /** Before anything is typed, the recent files; then the files the query found. */
  files: readonly FileRow[];
  /** The folders that match the words typed (listed after the files). */
  folders: readonly FolderRow[];
  /** Hidden files that match and are not listed: the number of the "+N hidden files match" row. */
  moreHidden: number;
  sections: readonly SectionRow[];
  tags: readonly TagRow[];
  commands: readonly CommandRow[];
};

function recentGroups(files: readonly FileRow[]): Group[] {
  return (["changed", "opened"] as const)
    .map((kind) => {
      const rows = files.filter((f) => f.group === kind);
      return { id: kind, title: kind, rows, total: rows.length, showAll: null } satisfies Group;
    })
    .filter((g) => g.rows.length > 0);
}

/** The one group of a single scope: no header, every result. `results` is how many of the rows are results. */
function single(kind: Exclude<Scope, "all">, rows: readonly Row[], results = rows.length): Group[] {
  if (rows.length === 0) return [];
  return [{ id: kind, title: null, rows: [...rows], total: results, showAll: null }];
}

/** The folders of the All scope; "Show all" goes to Files, where the folders follow the files. */
function foldersPart(rows: readonly FolderRow[], limit: number): Group | null {
  if (rows.length === 0) return null;
  const shown = rows.slice(0, limit);
  return {
    id: "folders",
    title: "folders",
    rows: shown,
    total: rows.length,
    showAll: rows.length > shown.length ? "files" : null,
  };
}

function part(kind: Exclude<Scope, "all">, rows: readonly Row[], limit: number): Group | null {
  if (rows.length === 0) return null;
  const shown = rows.slice(0, limit);
  return {
    id: kind,
    title: kind,
    rows: shown,
    total: rows.length,
    showAll: rows.length > shown.length ? kind : null,
  };
}

/**
 * What the box shows for a scope. All lists Files (5), Sections (5), Tags (3) and Commands (3) in that fixed order, so
 * the first row is the best file; a group with no results is left out. Before anything is typed it lists the recent
 * files, as Quick open does, and with only `tag:` filters just the files that have them.
 */
export function buildView(input: ViewInput): View {
  const { scope, idle, hasWords, files, folders, moreHidden, sections, tags, commands } = input;
  let groups: Group[];
  switch (scope) {
    case "all": {
      if (idle) {
        groups = recentGroups(files);
        break;
      }
      groups = [
        part("files", files, GROUP_LIMIT.files),
        foldersPart(folders, GROUP_LIMIT.folders),
        ...(hasWords
          ? [
              part("sections", sections, GROUP_LIMIT.sections),
              part("tags", tags, GROUP_LIMIT.tags),
              part("commands", commands, GROUP_LIMIT.commands),
            ]
          : []),
      ].filter((g): g is Group => g !== null);
      break;
    }
    case "files": {
      if (idle) {
        groups = recentGroups(files);
        break;
      }
      const more: MoreRow[] =
        moreHidden > 0 ? [{ type: "more", key: "more", count: moreHidden }] : [];
      groups = single("files", [...files, ...folders, ...more], files.length + folders.length);
      break;
    }
    case "sections":
      groups = single("sections", sections);
      break;
    case "tags":
      groups = single("tags", tags);
      break;
    case "commands":
      groups = single("commands", commands);
      break;
  }
  return { groups, rows: groups.flatMap((g) => g.rows) };
}

// ---- the keyboard -------------------------------------------------------------------------------------

/** The row after (1) or before (-1) the active one, wrapping around the ends; 0 for a list with no rows. */
export function moveActive(active: number, delta: 1 | -1, count: number): number {
  if (count <= 0) return 0;
  return (active + delta + count) % count;
}
