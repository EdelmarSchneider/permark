// Pure model of the search panel: grouping hits by file, ordering, safe snippets and the flat list
// used for keyboard navigation.

import { basename, folderLabel } from "./paths";
import type { SearchHit } from "./types";

export type HitGroup = {
  path: string;
  name: string;
  /** Folder shown next to the name; "" for the project root. */
  folder: string;
  best: number;
  hits: SearchHit[];
};

/** Groups by file; groups ordered by their best score, hits by score. */
export function groupHits(hits: SearchHit[]): HitGroup[] {
  const byPath = new Map<string, HitGroup>();
  for (const hit of hits) {
    let group = byPath.get(hit.path);
    if (!group) {
      group = {
        path: hit.path,
        name: basename(hit.path),
        folder: folderLabel(hit.path),
        best: hit.score,
        hits: [],
      };
      byPath.set(hit.path, group);
    }
    group.best = Math.max(group.best, hit.score);
    group.hits.push(hit);
  }
  const groups = [...byPath.values()];
  for (const group of groups) group.hits.sort((a, b) => b.score - a.score);
  return groups.sort((a, b) => b.best - a.best);
}

/** Keeps only `<mark>` tags from a backend snippet; any other tag-like text is escaped. */
export function sanitizeSnippet(snippet: string): string {
  return snippet.replace(/<(?!\/?mark>)/g, "&lt;");
}

/** Heading breadcrumb of a hit; a hit before the first heading shows the file name instead. */
export function crumbOf(hit: SearchHit): string[] {
  return hit.headingPath.length > 0 ? hit.headingPath : [basename(hit.path)];
}

export type ResultRow =
  { type: "group"; group: HitGroup } | { type: "hit"; group: HitGroup; hit: SearchHit };

/** The rows visible in the list (hits of collapsed groups are skipped), in keyboard order. */
export function flattenResults(groups: HitGroup[], collapsed: ReadonlySet<string>): ResultRow[] {
  const rows: ResultRow[] = [];
  for (const group of groups) {
    rows.push({ type: "group", group });
    if (!collapsed.has(group.path)) {
      for (const hit of group.hits) rows.push({ type: "hit", group, hit });
    }
  }
  return rows;
}

/** Stable identity of a hit row. */
export function hitKey(hit: SearchHit): string {
  return `${hit.path}:${hit.startLine}`;
}
