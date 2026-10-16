// Pure model of the tag UI: matching tag names while typing, the suggestions list of the tag editor,
// filtering the Tags view, and which tags are selected there.

import { joinList } from "./i18n";
import { foldChars } from "./text";
import { normalizeTag, normalizeTags } from "./tagText";
import type { FileMeta, IndexEntry, TagCount } from "./types";

/** A tag with the number of files that have it (what the backend and the local index both provide). */
export type NamedCount = Pick<TagCount, "name" | "count">;

// ---- matching -----------------------------------------------------------------------------------

/** How well a tag name matches what was typed; lower is better. */
export type MatchTier = 0 | 1 | 2 | 3 | 4;

export type TagMatch = {
  tag: NamedCount;
  tier: MatchTier;
  /** Indices into the tag name of the characters that matched (for bold rendering). */
  positions: number[];
};

const BOUNDARY = /[-_/.]/;

function range(start: number, length: number): number[] {
  return Array.from({ length }, (_, i) => start + i);
}

/**
 * Matches `query` against a tag name: the whole name (0), its start (1), the start of a word inside it
 * (2), anywhere in it (3), or the characters in order (4). Null when it does not match.
 */
export function matchTag(
  query: string,
  name: string,
): { tier: MatchTier; positions: number[] } | null {
  const q = foldChars(query.trim().toLowerCase());
  if (q === "") return { tier: 4, positions: [] };
  const n = foldChars(name);
  if (n === q) return { tier: 0, positions: range(0, q.length) };
  if (n.startsWith(q)) return { tier: 1, positions: range(0, q.length) };
  for (let i = 1; i < n.length; i++) {
    if (BOUNDARY.test(n[i - 1]) && n.startsWith(q, i)) {
      return { tier: 2, positions: range(i, q.length) };
    }
  }
  const at = n.indexOf(q);
  if (at >= 0) return { tier: 3, positions: range(at, q.length) };
  const positions: number[] = [];
  let from = 0;
  for (const ch of q) {
    const found = n.indexOf(ch, from);
    if (found < 0) return null;
    positions.push(found);
    from = found + 1;
  }
  return { tier: 4, positions };
}

/**
 * The tags that match `query`: better matches first, and among equal matches the tag on more files
 * first (then by name). An empty query lists every tag by file count.
 */
export function rankTags(
  query: string,
  tags: readonly NamedCount[],
  exclude: ReadonlySet<string> = new Set(),
): TagMatch[] {
  const out: TagMatch[] = [];
  for (const tag of tags) {
    if (exclude.has(tag.name)) continue;
    const match = matchTag(query, tag.name);
    if (match) out.push({ tag, ...match });
  }
  return out.sort(
    (a, b) => a.tier - b.tier || b.tag.count - a.tag.count || a.tag.name.localeCompare(b.tag.name),
  );
}

/** The tags of the Tags view that match its filter box, still by file count. */
export function filterTags<T extends NamedCount>(tags: readonly T[], filter: string): T[] {
  if (filter.trim() === "") return [...tags];
  return tags.filter((tag) => matchTag(filter, tag.name) !== null);
}

// ---- the tag editor's suggestions ---------------------------------------------------------------

export type Suggestion = { type: "tag"; match: TagMatch } | { type: "create"; name: string };

/**
 * What the tag box offers for the text typed so far: matching existing tags, then a "Create tag" row
 * unless the text already is a tag (on this file, or anywhere).
 */
export function suggest(
  typed: string,
  tags: readonly NamedCount[],
  onFile: ReadonlySet<string>,
  limit = 8,
): Suggestion[] {
  const name = normalizeTag(typed);
  const rows: Suggestion[] = rankTags(typed, tags, onFile)
    .slice(0, limit)
    .map((match) => ({ type: "tag", match }));
  const exists = name !== null && (onFile.has(name) || tags.some((t) => t.name === name));
  if (name !== null && !exists) rows.push({ type: "create", name });
  return rows;
}

// ---- the Tags view ------------------------------------------------------------------------------

export type TagSelection = { tags: string[]; untagged: boolean };

export const NO_SELECTION: TagSelection = { tags: [], untagged: false };

export function isEmptySelection(sel: TagSelection): boolean {
  return sel.tags.length === 0 && !sel.untagged;
}

/** A click: only this tag. */
export function selectOnly(tag: string): TagSelection {
  return { tags: [tag], untagged: false };
}

/** Ctrl+click or Space: add the tag to the selection, or take it out. Files must have all of them. */
export function toggleTag(sel: TagSelection, tag: string): TagSelection {
  const tags = sel.tags.includes(tag) ? sel.tags.filter((t) => t !== tag) : [...sel.tags, tag];
  return { tags, untagged: false };
}

/** "Untagged" stands for the files without tags, so it cannot combine with tags. */
export function toggleUntagged(sel: TagSelection, only: boolean): TagSelection {
  if (only) return { tags: [], untagged: true };
  return { tags: [], untagged: !sel.untagged };
}

/** What a tag selection is called in the file list heading: "spec + needs-review". */
export function selectionLabel(sel: TagSelection, untaggedLabel: string): string {
  return sel.untagged ? untaggedLabel : sel.tags.join(" + ");
}

/** Selected tags that no longer exist (renamed, or the last file lost it) are dropped. */
export function pruneSelection(
  sel: TagSelection,
  tags: readonly Pick<TagCount, "name">[],
): TagSelection {
  const alive = new Set(tags.map((t) => t.name));
  const kept = sel.tags.filter((t) => alive.has(t));
  return kept.length === sel.tags.length ? sel : { tags: kept, untagged: sel.untagged };
}

// ---- counts from the index ---------------------------------------------------------------------

/** Tags with their number of files, from the per-file index (most files first). */
export function countTags(entries: Iterable<IndexEntry>): NamedCount[] {
  const counts = new Map<string, number>();
  for (const entry of entries) {
    for (const tag of entry.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
  }
  return [...counts]
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
}

/** The line shown for a tag that no file has: "No tag 'x'. Tags: spec, feed, ..." */
export function knownTagsHint(counts: readonly Pick<TagCount, "name">[], max = 6): string {
  const names = counts.slice(0, max).map((t) => t.name);
  return counts.length > max ? `${joinList(names)}...` : joinList(names);
}

// ---- tooltips -----------------------------------------------------------------------------------

/**
 * The tooltip of a file in the tree: its path, then the description and the tags line when it has
 * any. `tagsLine` words the line (it is a translated string with the list in it).
 */
export function treeTooltip(
  path: string,
  entry: Pick<IndexEntry, "tags" | "description"> | undefined,
  tagsLine: (list: string) => string,
): string | undefined {
  if (!entry) return undefined;
  const lines = [path];
  if (entry.description !== "") lines.push(entry.description);
  if (entry.tags.length > 0) lines.push(tagsLine(joinList(entry.tags)));
  return lines.length > 1 ? lines.join("\n") : undefined;
}

// ---- tags of the open document ---------------------------------------------------------------

/**
 * The tags of a file as they will be once the app's tags are `names`: the front matter's stay, and
 * the app's follow the list. Used to show a change before the backend has answered, so two quick
 * changes build on each other.
 */
export function withAppTags(meta: FileMeta, names: readonly string[]): FileMeta {
  const app = normalizeTags(names);
  const fromFile = meta.tags
    .filter((tag) => tag.file)
    .map((tag) => ({ ...tag, app: app.includes(tag.name) }));
  const appOnly = app
    .filter((name) => !fromFile.some((tag) => tag.name === name))
    .map((name) => ({ name, file: false, app: true }));
  return { ...meta, tags: [...fromFile, ...appOnly] };
}
