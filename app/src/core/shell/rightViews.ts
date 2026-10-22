// The views of the right column (the outline and the right panels plugins add): how they are arranged (groups one
// above the other, a group with several views = tabs; app-wide, saved in the settings) and which are open + selected
// (per project, saved in its session). Pure: parse what was saved, work out what is drawn, the menu's moves. Nothing
// here assumes two groups or the right region only: a group list is a list (docking later = more regions).

/** The core view: the outline of the document. */
export const OUTLINE = "outline";
/** The live feed's panel: the default arrangement puts it under the outline (its plugin may be off). */
export const LIVE_VIEW = "live-feed.panel";
/** At most this many groups one above the other. */
export const MAX_GROUPS = 3;
/** A group's saved share never goes under this (it would vanish). */
export const MIN_SHARE = 0.05;
/** A drawn group is never squeezed under this height (header + 3 rows). */
export const MIN_GROUP_PX = 120;

export type ViewGroup = { readonly views: readonly string[]; readonly share: number };
export type RightViews = { readonly v: 1; readonly groups: readonly ViewGroup[] };

/** Stacked: the outline on top, Live changes below, half each. */
export function defaultRightViews(): RightViews {
  return {
    v: 1,
    groups: [
      { views: [OUTLINE], share: 0.5 },
      { views: [LIVE_VIEW], share: 0.5 },
    ],
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Shares clamped (>= `MIN_SHARE`; not a number = the minimum) and made to sum 1. */
export function normalize(groups: readonly ViewGroup[]): ViewGroup[] {
  const clamped = groups.map((g) => ({
    views: g.views,
    share: Number.isFinite(g.share) ? Math.max(MIN_SHARE, g.share) : MIN_SHARE,
  }));
  const sum = clamped.reduce((total, g) => total + g.share, 0);
  return clamped.map((g) => ({ views: g.views, share: g.share / sum }));
}

/**
 * What was saved, made safe: absent | damaged | another version -> the default; a view listed twice -> its first
 * place; more than `MAX_GROUPS` groups -> the extra ones join the last allowed group as tabs; empty groups dropped;
 * shares normalized. Ids of views not known now are kept (a plugin that is off finds its place again).
 */
export function parseRightViews(raw: unknown): RightViews {
  if (!isRecord(raw) || raw["v"] !== 1 || !Array.isArray(raw["groups"])) return defaultRightViews();
  const seen = new Set<string>();
  const groups: { views: string[]; share: number }[] = [];
  for (const group of raw["groups"]) {
    if (!isRecord(group) || !Array.isArray(group["views"])) continue;
    const views: string[] = [];
    for (const id of group["views"]) {
      if (typeof id !== "string" || id === "" || seen.has(id)) continue;
      seen.add(id);
      views.push(id);
    }
    if (views.length === 0) continue;
    const share = group["share"];
    groups.push({ views, share: typeof share === "number" ? share : 0 });
  }
  if (groups.length === 0) return defaultRightViews();
  const kept = groups.slice(0, MAX_GROUPS);
  const last = kept[kept.length - 1];
  for (const extra of groups.slice(MAX_GROUPS)) {
    if (last === undefined) break;
    last.views.push(...extra.views);
  }
  return { v: 1, groups: normalize(kept) };
}

/** Views the arrangement does not list yet (a plugin turned on for the first time) join the last group as tabs. */
export function withViews(layout: RightViews, ids: readonly string[]): RightViews {
  const listed = new Set(layout.groups.flatMap((g) => g.views));
  const missing = ids.filter((id) => !listed.has(id));
  if (missing.length === 0) return layout;
  const groups = layout.groups.map((g, at) =>
    at === layout.groups.length - 1 ? { views: [...g.views, ...missing], share: g.share } : g,
  );
  return { v: 1, groups: groups.length > 0 ? groups : [{ views: missing, share: 1 }] };
}

/** A group as drawn: its place in the arrangement, its present views in order, its share of the drawn ones. */
export type DrawnGroup = {
  readonly at: number;
  readonly views: readonly string[];
  readonly share: number;
};

/** The groups drawn: those with a present view; their shares renormalized over them (the saved ones untouched). */
export function drawnGroups(layout: RightViews, present: (id: string) => boolean): DrawnGroup[] {
  const drawn = layout.groups
    .map((g, at) => ({ at, views: g.views.filter(present), share: g.share }))
    .filter((g) => g.views.length > 0);
  const sum = drawn.reduce((total, g) => total + g.share, 0);
  return drawn.map((g) => ({ ...g, share: sum > 0 ? g.share / sum : 1 / drawn.length }));
}

/** The arrangement's group that holds `id` (-1: none). */
export function groupOf(layout: RightViews, id: string): number {
  return layout.groups.findIndex((g) => g.views.includes(id));
}

/** The drawn group above (`-1`) | below (`1`) the one of `id`, as an arrangement index (-1: none). */
function neighbor(
  layout: RightViews,
  id: string,
  direction: -1 | 1,
  drawnAt: readonly number[],
): number {
  const at = groupOf(layout, id);
  const pos = drawnAt.indexOf(at);
  if (at < 0 || pos < 0) return -1;
  return drawnAt[pos + direction] ?? -1;
}

/** Move up | down can act (a drawn group above | below). */
export function canMove(
  layout: RightViews,
  id: string,
  direction: -1 | 1,
  drawnAt: readonly number[],
): boolean {
  return neighbor(layout, id, direction, drawnAt) >= 0;
}

/** The group of `id` changes place with the drawn group above | below (shares travel with their groups). */
export function moveView(
  layout: RightViews,
  id: string,
  direction: -1 | 1,
  drawnAt: readonly number[],
): RightViews {
  const at = groupOf(layout, id);
  const other = neighbor(layout, id, direction, drawnAt);
  if (at < 0 || other < 0) return layout;
  const groups = [...layout.groups];
  const mine = groups[at];
  const theirs = groups[other];
  if (mine === undefined || theirs === undefined) return layout;
  groups[at] = theirs;
  groups[other] = mine;
  return { v: 1, groups };
}

/** Show as tab can act: another group is drawn. */
export function canJoin(layout: RightViews, id: string, drawnAt: readonly number[]): boolean {
  return neighbor(layout, id, -1, drawnAt) >= 0 || neighbor(layout, id, 1, drawnAt) >= 0;
}

/**
 * `id` joins the drawn group above as its last tab (the top one: the group below). A group left empty goes and its
 * share goes to the group joined.
 */
export function joinAsTab(layout: RightViews, id: string, drawnAt: readonly number[]): RightViews {
  const at = groupOf(layout, id);
  const above = neighbor(layout, id, -1, drawnAt);
  const target = above >= 0 ? above : neighbor(layout, id, 1, drawnAt);
  const from = layout.groups[at];
  if (at < 0 || target < 0 || from === undefined) return layout;
  const left = from.views.filter((v) => v !== id);
  const groups = layout.groups.map((g, i) => {
    if (i === target) {
      return { views: [...g.views, id], share: g.share + (left.length === 0 ? from.share : 0) };
    }
    if (i === at) return { views: left, share: g.share };
    return g;
  });
  return { v: 1, groups: normalize(groups.filter((g) => g.views.length > 0)) };
}

/** Show above | below can act: `id` shares its group, and there is room for one more group. */
export function canSplit(layout: RightViews, id: string): boolean {
  const group = layout.groups[groupOf(layout, id)];
  return group !== undefined && group.views.length > 1 && layout.groups.length < MAX_GROUPS;
}

/** `id` leaves its tab group for a new group right above | below it; each gets half of the old share. */
export function splitOut(layout: RightViews, id: string, where: "above" | "below"): RightViews {
  if (!canSplit(layout, id)) return layout;
  const at = groupOf(layout, id);
  const group = layout.groups[at];
  if (group === undefined) return layout;
  const half = group.share / 2;
  const alone = { views: [id], share: half };
  const rest = { views: group.views.filter((v) => v !== id), share: half };
  const groups = [...layout.groups];
  groups.splice(at, 1, ...(where === "above" ? [alone, rest] : [rest, alone]));
  return { v: 1, groups };
}

/** Two groups share their room anew: `above` takes `ratio` (0..1) of what both had together. */
export function resizePair(
  layout: RightViews,
  above: number,
  below: number,
  ratio: number,
): RightViews {
  const a = layout.groups[above];
  const b = layout.groups[below];
  if (a === undefined || b === undefined || !Number.isFinite(ratio)) return layout;
  const both = a.share + b.share;
  const share = Math.min(1, Math.max(0, ratio)) * both;
  const groups = layout.groups.map((g, i) =>
    i === above
      ? { views: g.views, share }
      : i === below
        ? { views: g.views, share: both - share }
        : g,
  );
  return { v: 1, groups };
}

/**
 * Heights (px) for drawn shares in `total` px, each >= `min`; `null` when even the minimums do not fit (the column
 * then draws its groups as one tab group).
 */
export function fitHeights(total: number, shares: readonly number[], min: number): number[] | null {
  if (shares.length === 0) return [];
  if (!Number.isFinite(total) || total < shares.length * min) return null;
  const fixed = new Set<number>();
  let heights = shares.map((s) => s * total);
  for (;;) {
    const room = total - fixed.size * min;
    const free = shares.reduce((sum, s, i) => (fixed.has(i) ? sum : sum + s), 0);
    heights = shares.map((s, i) => (fixed.has(i) ? min : free > 0 ? (s / free) * room : 0));
    const under = heights.findIndex((h, i) => !fixed.has(i) && h < min);
    if (under < 0) break;
    fixed.add(under);
  }
  const whole = heights.map((h) => Math.floor(h));
  const rest = total - whole.reduce((sum, h) => sum + h, 0);
  whole[whole.length - 1] = (whole[whole.length - 1] ?? 0) + rest;
  return whole;
}

/** The selected view of a tab group: the first of `selected` among its present views, else the first present. */
export function selectedIn(views: readonly string[], selected: readonly string[]): string | null {
  return selected.find((id) => views.includes(id)) ?? views[0] ?? null;
}

/** `id` becomes the selected view of its group (`group`: every view of that group). */
export function select(
  selected: readonly string[],
  group: readonly string[],
  id: string,
): string[] {
  return [id, ...selected.filter((s) => s !== id && !group.includes(s))];
}

/** What a project keeps: which views are open (absent = the view's default) and the selected tab of each group. */
export type ProjectViews = {
  readonly v: 1;
  readonly open: Readonly<Record<string, boolean>>;
  readonly selected: readonly string[];
};

export function emptyProjectViews(): ProjectViews {
  return { v: 1, open: {}, selected: [] };
}

/** A project's saved views, made safe: absent | damaged | another version -> nothing chosen yet. */
export function parseProjectViews(raw: unknown): ProjectViews {
  if (!isRecord(raw) || raw["v"] !== 1) return emptyProjectViews();
  const open: Record<string, boolean> = {};
  const rawOpen = raw["open"];
  if (isRecord(rawOpen)) {
    for (const [id, value] of Object.entries(rawOpen)) {
      if (typeof value === "boolean") open[id] = value;
    }
  }
  const rawSelected = raw["selected"];
  const selected = Array.isArray(rawSelected)
    ? rawSelected.filter((id): id is string => typeof id === "string" && id !== "")
    : [];
  return { v: 1, open, selected: [...new Set(selected)] };
}
