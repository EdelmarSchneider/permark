// The filter as the UI edits and remembers it. Running it is the backend's job (one implementation, tested there);
// this file holds the preset table the popover shows, the default, and the check of a remembered filter.

import type { Category, FilterShow, JsonValue, TreeFilter } from "../../api";

export const FILTER_SHOWS: readonly FilterShow[] = [
  "documents",
  "docs-and-assets",
  "everything",
  "custom",
];

export const CATEGORIES: readonly Category[] = [
  "markdown",
  "text",
  "config",
  "data",
  "image",
  "audio",
  "video",
  "office",
  "other",
];

const PRESETS: Record<Exclude<FilterShow, "custom">, readonly Category[]> = {
  documents: ["markdown", "text"],
  "docs-and-assets": ["markdown", "text", "config", "data", "image", "audio", "video"],
  everything: CATEGORIES,
};

/** The categories a preset shows; `custom` starts from nothing. */
export function categoriesOf(show: FilterShow): Category[] {
  return show === "custom" ? [] : [...PRESETS[show]];
}

export const DEFAULT_FILTER: TreeFilter = {
  show: "docs-and-assets",
  categories: categoriesOf("docs-and-assets"),
  patterns: "",
  assetsNearDocsOnly: true,
  hideEmptyDirs: true,
  showHidden: false,
  showIgnored: false,
};

/** Switching to custom keeps what the previous preset showed, so the list does not jump. */
export function withShow(filter: TreeFilter, show: FilterShow): TreeFilter {
  if (show === filter.show) return filter;
  const categories =
    show === "custom" && filter.show !== "custom" ? categoriesOf(filter.show) : filter.categories;
  return { ...filter, show, categories };
}

export function isFilterShow(value: unknown): value is FilterShow {
  return FILTER_SHOWS.some((s) => s === value);
}

export function isCategory(value: unknown): value is Category {
  return CATEGORIES.some((c) => c === value);
}

/** A remembered filter, checked field by field. `null` when it is missing or not a filter. */
export function normalizeFilter(raw: unknown): TreeFilter | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r: Record<string, unknown> = { ...raw };
  const show = r["show"];
  if (!isFilterShow(show)) return null;
  const categories = Array.isArray(r["categories"])
    ? [...new Set(r["categories"].filter(isCategory))]
    : [];
  return {
    show,
    categories,
    patterns: typeof r["patterns"] === "string" ? r["patterns"] : "",
    assetsNearDocsOnly: r["assetsNearDocsOnly"] !== false,
    hideEmptyDirs: r["hideEmptyDirs"] !== false,
    showHidden: r["showHidden"] === true,
    showIgnored: r["showIgnored"] === true,
  };
}

/** The filter as the project session stores it. */
export function filterToJson(filter: TreeFilter): JsonValue {
  return {
    show: filter.show,
    categories: [...filter.categories],
    patterns: filter.patterns,
    assetsNearDocsOnly: filter.assetsNearDocsOnly,
    hideEmptyDirs: filter.hideEmptyDirs,
    showHidden: filter.showHidden,
    showIgnored: filter.showIgnored,
  };
}

/** Whether two filters are the same setting (the popover marks "project default" with it). */
export function sameFilter(a: TreeFilter, b: TreeFilter): boolean {
  return (
    a.show === b.show &&
    a.patterns === b.patterns &&
    a.assetsNearDocsOnly === b.assetsNearDocsOnly &&
    a.hideEmptyDirs === b.hideEmptyDirs &&
    a.showHidden === b.showHidden &&
    a.showIgnored === b.showIgnored &&
    [...a.categories].sort().join() === [...b.categories].sort().join()
  );
}
