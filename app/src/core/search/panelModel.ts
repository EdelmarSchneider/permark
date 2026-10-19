// The Search panel's results: hits grouped by document, and the flat list of rows the keyboard walks. Pure.

import type { SectionHit } from "../../api";

export type HitGroup = { path: string; hits: readonly SectionHit[] };

/** Documents in the order of their best hit; each document's hits keep the ranking order. */
export function groupHits(hits: readonly SectionHit[]): HitGroup[] {
  const groups = new Map<string, SectionHit[]>();
  for (const hit of hits) {
    const list = groups.get(hit.path);
    if (list) list.push(hit);
    else groups.set(hit.path, [hit]);
  }
  return [...groups].map(([path, list]) => ({ path, hits: list }));
}

export type PanelRow =
  | { type: "file"; key: string; path: string; count: number }
  | { type: "hit"; key: string; hit: SectionHit };

export function hitKey(hit: SectionHit): string {
  return `${hit.path}:${hit.line}`;
}

/** Header row per document, then its hits unless the document is folded. */
export function flattenRows(
  groups: readonly HitGroup[],
  collapsed: ReadonlySet<string>,
): PanelRow[] {
  const rows: PanelRow[] = [];
  for (const group of groups) {
    rows.push({
      type: "file",
      key: `file:${group.path}`,
      path: group.path,
      count: group.hits.length,
    });
    if (collapsed.has(group.path)) continue;
    for (const hit of group.hits) rows.push({ type: "hit", key: hitKey(hit), hit });
  }
  return rows;
}
