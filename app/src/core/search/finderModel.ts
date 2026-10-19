// What the Search everywhere box shows: the scopes of each face of the box, the prefixes that switch them, the
// rows each scope lists and how All groups them. Pure; `finder.svelte.ts` feeds it and draws what it returns.

import type {
  Category,
  FileHit,
  FolderHit,
  OutlineItem,
  Recent,
  SectionHit,
  SnippetPart,
} from "../../api";
import type { CommandEntry } from "../commands/commands";
import { fuzzyMatch } from "./fuzzy";

export type Preset = "quick" | "everywhere" | "commands";
export type Scope = "all" | "files" | "sections" | "commands";

/** Rows each group of All lists; the rest is behind "Show all". */
export const GROUP_LIMIT = { files: 5, folders: 3, sections: 5, commands: 3 } as const;
/** Recent files of each kind an empty box lists. */
export const RECENT_LIMIT = 8;
/** Most rows a single scope lists. */
export const RESULT_LIMIT = 50;

/** The scopes a face of the box offers, in tab order. */
export function scopesFor(preset: Preset, singleFile: boolean): readonly Scope[] {
  switch (preset) {
    case "quick":
      return ["files"];
    case "commands":
      return ["commands"];
    case "everywhere":
      return singleFile ? ["sections", "commands"] : ["all", "files", "sections", "commands"];
    default:
      return preset satisfies never;
  }
}

/** The scope the box starts in. */
export function firstScope(scopes: readonly Scope[]): Scope {
  return scopes[0] ?? "commands";
}

/** The scope after (1) or before (-1) the current one, wrapping around the ends. */
export function stepScope(scopes: readonly Scope[], current: Scope, delta: 1 | -1): Scope {
  if (scopes.length === 0) return current;
  const at = scopes.indexOf(current);
  const next = (Math.max(at, 0) + delta + scopes.length) % scopes.length;
  return scopes[next] ?? current;
}

/**
 * A prefix typed as the first character switches the scope and leaves the text: `>` is Commands, `#` is Sections.
 * A prefix whose scope the box does not offer is ordinary text.
 */
export function applyPrefix(
  scopes: readonly Scope[],
  scope: Scope,
  text: string,
): { scope: Scope; text: string } {
  const prefixed: Scope | null = text.startsWith(">")
    ? "commands"
    : text.startsWith("#")
      ? "sections"
      : null;
  if (prefixed === null || !scopes.includes(prefixed) || scopes.length < 2) return { scope, text };
  return { scope: prefixed, text: text.slice(1) };
}

// ---- rows ----------------------------------------------------------------------------------------------------------

export type FileRow = {
  type: "file";
  key: string;
  path: string;
  category: Category;
  positions: readonly number[];
};
export type FolderRow = { type: "folder"; key: string; path: string; positions: readonly number[] };
export type SectionRow = {
  type: "section";
  key: string;
  path: string;
  headingPath: readonly string[];
  anchor: string | null;
  line: number;
  snippet: readonly SnippetPart[];
};
export type CommandRow = {
  type: "command";
  key: string;
  entry: CommandEntry;
  positions: readonly number[];
};
/** "Show all (n)": the rest of a group, in its own scope. */
export type MoreRow = { type: "more"; key: string; scope: Scope; count: number };
export type Row = FileRow | FolderRow | SectionRow | CommandRow | MoreRow;

export type GroupTitle = "opened" | "changed" | "files" | "folders" | "sections" | "commands";
export type Group = { title: GroupTitle; rows: readonly Row[] };

/** What the box has to show, besides the words. */
export type Data = {
  files: readonly FileHit[];
  folders: readonly FolderHit[];
  sections: readonly SectionHit[];
  recent: Recent | null;
};

export const NO_DATA: Data = { files: [], folders: [], sections: [], recent: null };

export type ViewInput = {
  preset: Preset;
  scope: Scope;
  query: string;
  data: Data;
  /** Commands matched by the query (all of them for an empty query), best first. */
  commands: readonly { entry: CommandEntry; positions: readonly number[] }[];
  /** The headings of the open document, as rows. */
  outline: readonly SectionRow[];
  /** The window shows a single file: Sections come from the open document only. */
  singleFile: boolean;
};

export type EmptyState = "none" | "noMatch";

export type View = {
  groups: readonly Group[];
  /** Every row in keyboard order. */
  rows: readonly Row[];
  /** `noMatch`: something was typed and nothing matches. */
  empty: EmptyState;
};

export function fileRow(hit: FileHit, prefix = ""): FileRow {
  return {
    type: "file",
    key: `${prefix}file:${hit.path}`,
    path: hit.path,
    category: hit.category,
    positions: hit.positions,
  };
}

function folderRow(hit: FolderHit): FolderRow {
  return { type: "folder", key: `folder:${hit.path}`, path: hit.path, positions: hit.positions };
}

export function sectionRow(hit: SectionHit): SectionRow {
  return {
    type: "section",
    key: `section:${hit.path}:${hit.line}`,
    path: hit.path,
    headingPath: hit.headingPath,
    anchor: hit.anchor,
    line: hit.line,
    snippet: hit.snippet,
  };
}

/** The headings of a document as section rows, each with the headings above it. */
export function outlineRows(path: string, outline: readonly OutlineItem[]): SectionRow[] {
  const stack: { level: number; label: string }[] = [];
  return outline.map((item) => {
    while (stack.length > 0 && (stack[stack.length - 1]?.level ?? 0) >= item.level) stack.pop();
    stack.push({ level: item.level, label: item.label });
    return {
      type: "section",
      key: `outline:${item.id}:${item.line}`,
      path,
      headingPath: stack.map((s) => s.label),
      anchor: item.id,
      line: item.line,
      snippet: [],
    };
  });
}

/** The headings whose last label matches the query (local filter of the open document), best first. */
export function filterOutline(rows: readonly SectionRow[], query: string): SectionRow[] {
  const ranked: { row: SectionRow; score: number }[] = [];
  for (const row of rows) {
    const found = fuzzyMatch(query, row.headingPath.at(-1) ?? "");
    if (found) ranked.push({ row, score: found.score });
  }
  ranked.sort((a, b) => b.score - a.score);
  return ranked.map((r) => r.row);
}

function commandRows(input: ViewInput): CommandRow[] {
  return input.commands.map(({ entry, positions }) => ({
    type: "command",
    key: `command:${entry.command.id}`,
    entry,
    positions,
  }));
}

function recentGroups(recent: Recent | null): Group[] {
  if (recent === null) return [];
  const opened = recent.opened.slice(0, RECENT_LIMIT).map((hit) => fileRow(hit, "opened:"));
  const seen = new Set(recent.opened.slice(0, RECENT_LIMIT).map((hit) => hit.path));
  const changed = recent.changed
    .filter((hit) => !seen.has(hit.path))
    .slice(0, RECENT_LIMIT)
    .map((hit) => fileRow(hit, "changed:"));
  const groups: Group[] = [];
  if (opened.length > 0) groups.push({ title: "opened", rows: opened });
  if (changed.length > 0) groups.push({ title: "changed", rows: changed });
  return groups;
}

function capped<T extends Row>(rows: readonly T[], limit: number, scope: Scope): Row[] {
  if (rows.length <= limit) return [...rows];
  return [
    ...rows.slice(0, limit),
    { type: "more", key: `more:${scope}`, scope, count: rows.length - limit },
  ];
}

function nonEmpty(groups: Group[]): Group[] {
  return groups.filter((g) => g.rows.length > 0);
}

/** What the box shows for a scope. */
export function buildView(input: ViewInput): View {
  const typed = input.query.trim() !== "";
  const files = input.data.files.map((hit) => fileRow(hit));
  const folders = input.data.folders.map(folderRow);
  const sections = input.data.sections.map(sectionRow);
  let groups: Group[];
  switch (input.scope) {
    case "all":
      groups = typed
        ? nonEmpty([
            { title: "files", rows: capped(files, GROUP_LIMIT.files, "files") },
            { title: "folders", rows: capped(folders, GROUP_LIMIT.folders, "files") },
            { title: "sections", rows: capped(sections, GROUP_LIMIT.sections, "sections") },
            {
              title: "commands",
              rows: capped(commandRows(input), GROUP_LIMIT.commands, "commands"),
            },
          ])
        : recentGroups(input.data.recent);
      break;
    case "files":
      groups = typed
        ? nonEmpty([
            { title: "files", rows: files.slice(0, RESULT_LIMIT) },
            { title: "folders", rows: input.preset === "quick" ? [] : folders },
          ])
        : recentGroups(input.data.recent);
      break;
    case "sections": {
      const rows = input.singleFile || !typed ? sectionsLocal(input, typed) : sections;
      groups = nonEmpty([{ title: "sections", rows: rows.slice(0, RESULT_LIMIT) }]);
      break;
    }
    case "commands":
      groups = nonEmpty([{ title: "commands", rows: commandRows(input) }]);
      break;
    default:
      groups = input.scope satisfies never;
  }
  const rows = groups.flatMap((g) => g.rows);
  return { groups, rows, empty: typed && rows.length === 0 ? "noMatch" : "none" };
}

function sectionsLocal(input: ViewInput, typed: boolean): readonly SectionRow[] {
  return typed ? filterOutline(input.outline, input.query) : input.outline;
}

/** The row after (1) or before (-1) the active one, wrapping around the ends; 0 for a list with no rows. */
export function moveActive(active: number, delta: 1 | -1, count: number): number {
  if (count === 0) return 0;
  return (active + delta + count) % count;
}
