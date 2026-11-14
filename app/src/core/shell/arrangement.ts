// The views of the window (Files, Search, the outline, the panels plugins add): how they are arranged in three regions
// (left | right | bottom; per region a list of groups along its stack, a group with several views = tabs; app-wide,
// saved in the settings) and which are open + selected (per project, saved in its session). Pure: parse what was
// saved, work out what is drawn, the moves. A region's groups are one list whatever the region: the same functions
// act in each; only the direction of the stack differs (`axisOf`).

/** The core views of the left side: the project's files, the search. */
export const FILES = "files";
export const SEARCH = "search";
/** The core view: the outline of the document. */
export const OUTLINE = "outline";
/** The live feed's panel: the default arrangement puts it under the outline (its plugin may be off). */
export const LIVE_VIEW = "live-feed.panel";
/** At most this many groups in a region. */
export const MAX_GROUPS = 3;
/** A group's saved share never goes under this (it would vanish). */
export const MIN_SHARE = 0.05;
/** A drawn group is never squeezed under this along the stack: a view's header and at least two of its rows. */
export const MIN_GROUP_PX = 210;
/** The divider's track between two drawn groups: its own room, so it never covers a row of a view. */
export const DIVIDER_PX = 7;
/** The height of a group's tab strip: the region draws it this high (its tabs' own height, 38.4 px, as whole pixels). */
export const STRIP_PX = 38;

export type Region = "left" | "right" | "bottom";
/** Every region, in the order a view listed twice keeps its place. */
export const REGIONS: readonly Region[] = ["left", "right", "bottom"];
/** The direction a region stacks its groups: one above the other, or side by side. */
export type Axis = "vertical" | "horizontal";

export type ViewGroup = { readonly views: readonly string[]; readonly share: number };
/** What 0.11 saved: the right column alone. */
export type RightViews = { readonly v: 1; readonly groups: readonly ViewGroup[] };
export type Arrangement = {
  readonly v: 2;
  readonly left: readonly ViewGroup[];
  readonly right: readonly ViewGroup[];
  readonly bottom: readonly ViewGroup[];
};

export function axisOf(region: Region): Axis {
  switch (region) {
    case "left":
    case "right":
      return "vertical";
    case "bottom":
      return "horizontal";
    default:
      return region satisfies never;
  }
}

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

/** Files and Search as tabs on the left, the outline over Live changes on the right, nothing at the bottom. */
export function defaultArrangement(): Arrangement {
  return {
    v: 2,
    left: [{ views: [FILES, SEARCH], share: 1 }],
    right: defaultRightViews().groups,
    bottom: [],
  };
}

/** A saved value is a table (not a list, not a plain value). */
export function isRecord(value: unknown): value is Record<string, unknown> {
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
 * One saved group list, made safe: a view in `seen` (listed before) is skipped; more than `MAX_GROUPS` groups -> the
 * extra ones join the last allowed group as tabs; empty groups dropped; shares normalized. Ids of views not known now
 * are kept (a plugin that is off finds its place again).
 */
function safeGroups(raw: readonly unknown[], seen: Set<string>): ViewGroup[] {
  const groups: { views: string[]; share: number }[] = [];
  for (const group of raw) {
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
  const kept = groups.slice(0, MAX_GROUPS);
  const last = kept[kept.length - 1];
  for (const extra of groups.slice(MAX_GROUPS)) {
    if (last === undefined) break;
    last.views.push(...extra.views);
  }
  return normalize(kept);
}

/**
 * The right column 0.11 saved, made safe (`safeGroups`): absent | damaged | another version | nothing usable -> the
 * default.
 */
export function parseRightViews(raw: unknown): RightViews {
  if (!isRecord(raw) || raw["v"] !== 1 || !Array.isArray(raw["groups"])) return defaultRightViews();
  const groups = safeGroups(raw["groups"], new Set());
  return groups.length === 0 ? defaultRightViews() : { v: 1, groups };
}

/**
 * One saved arrangement (the window's own, a named layout's), made safe: readable (this version, three lists) ->
 * each region by `safeGroups`, a view listed twice anywhere keeps its first place (`REGIONS` order), an empty region
 * stays empty. Anything else (absent, damaged, another version): `null`.
 */
export function readArrangement(panels: unknown): Arrangement | null {
  if (!isRecord(panels) || panels["v"] !== 2) return null;
  const { left, right, bottom } = panels;
  if (!Array.isArray(left) || !Array.isArray(right) || !Array.isArray(bottom)) return null;
  const seen = new Set<string>();
  return {
    v: 2,
    left: safeGroups(left, seen),
    right: safeGroups(right, seen),
    bottom: safeGroups(bottom, seen),
  };
}

/**
 * The window's saved arrangement, made safe: `panels` when readable (`readArrangement`), else the default left, the
 * right column as 0.11 saved it (`rightViews`), no bottom.
 */
export function parseArrangement(panels: unknown, rightViews: unknown): Arrangement {
  return readArrangement(panels) ?? fromRightViews(rightViews);
}

function fromRightViews(rightViews: unknown): Arrangement {
  return { ...defaultArrangement(), right: parseRightViews(rightViews).groups };
}

/** The region that lists `id` (`null`: none). */
export function regionOf(arr: Arrangement, id: string): Region | null {
  return REGIONS.find((region) => groupOf(arr[region], id) >= 0) ?? null;
}

/** `arr` with `groups` as its `region` (the same groups: the same object). */
export function withRegion(
  arr: Arrangement,
  region: Region,
  groups: readonly ViewGroup[],
): Arrangement {
  return groups === arr[region] ? arr : { ...arr, [region]: groups };
}

/** A move inside one region: `op` gets the groups of the region that lists `id` and answers the new ones. */
export function inRegion(
  arr: Arrangement,
  id: string,
  op: (groups: readonly ViewGroup[]) => readonly ViewGroup[],
): Arrangement {
  const region = regionOf(arr, id);
  return region === null ? arr : withRegion(arr, region, op(arr[region]));
}

/**
 * Views no region lists yet (a plugin turned on for the first time) join the last group of their default region as
 * tabs (an empty region: its first group).
 */
export function withViews(
  arr: Arrangement,
  views: readonly { readonly id: string; readonly region: Region }[],
): Arrangement {
  return views.reduce(
    (next, view) => (regionOf(next, view.id) === null ? moveTo(next, view.id, view.region) : next),
    arr,
  );
}

/** Two shares are the same but for rounding. */
const SHARE_EPSILON = 1e-6;

/**
 * `arr` is the default arrangement: in every region the same groups, in the same order, with the same shares, once
 * both list `views` (the views known now: one a plugin adds joins its default region in both).
 */
export function isDefault(
  arr: Arrangement,
  views: readonly { readonly id: string; readonly region: Region }[],
): boolean {
  const mine = withViews(arr, views);
  const base = withViews(defaultArrangement(), views);
  return REGIONS.every((region) => {
    const groups = mine[region];
    const defaults = base[region];
    return (
      groups.length === defaults.length &&
      groups.every((group, at) => {
        const other = defaults[at];
        return (
          other !== undefined &&
          Math.abs(group.share - other.share) < SHARE_EPSILON &&
          group.views.length === other.views.length &&
          group.views.every((id, i) => id === other.views[i])
        );
      })
    );
  });
}

/** A group as drawn: its place in the region, its present views in order, its share of the drawn ones. */
export type DrawnGroup = {
  readonly at: number;
  readonly views: readonly string[];
  readonly share: number;
};

/** The groups drawn: those with a present view; their shares renormalized over them (the saved ones untouched). */
export function drawnGroups(
  groups: readonly ViewGroup[],
  present: (id: string) => boolean,
): DrawnGroup[] {
  const drawn = groups
    .map((g, at) => ({ at, views: g.views.filter(present), share: g.share }))
    .filter((g) => g.views.length > 0);
  const sum = drawn.reduce((total, g) => total + g.share, 0);
  return drawn.map((g) => ({ ...g, share: sum > 0 ? g.share / sum : 1 / drawn.length }));
}

/** The group of the region that holds `id` (-1: none). */
export function groupOf(groups: readonly ViewGroup[], id: string): number {
  return groups.findIndex((g) => g.views.includes(id));
}

/** The drawn group before (`-1`) | after (`1`) the one of `id` along the stack, as a region index (-1: none). */
function neighbor(
  groups: readonly ViewGroup[],
  id: string,
  direction: -1 | 1,
  drawnAt: readonly number[],
): number {
  const at = groupOf(groups, id);
  const pos = drawnAt.indexOf(at);
  if (at < 0 || pos < 0) return -1;
  return drawnAt[pos + direction] ?? -1;
}

/** Move before | after can act (a drawn group before | after). */
export function canMove(
  groups: readonly ViewGroup[],
  id: string,
  direction: -1 | 1,
  drawnAt: readonly number[],
): boolean {
  return neighbor(groups, id, direction, drawnAt) >= 0;
}

/** The group of `id` changes place with the drawn group before | after (shares travel with their groups). */
export function moveView(
  groups: readonly ViewGroup[],
  id: string,
  direction: -1 | 1,
  drawnAt: readonly number[],
): readonly ViewGroup[] {
  const at = groupOf(groups, id);
  const other = neighbor(groups, id, direction, drawnAt);
  const mine = groups[at];
  const theirs = groups[other];
  if (mine === undefined || theirs === undefined) return groups;
  const next = [...groups];
  next[at] = theirs;
  next[other] = mine;
  return next;
}

/** Show as tab can act: another group is drawn. */
export function canJoin(
  groups: readonly ViewGroup[],
  id: string,
  drawnAt: readonly number[],
): boolean {
  return neighbor(groups, id, -1, drawnAt) >= 0 || neighbor(groups, id, 1, drawnAt) >= 0;
}

/**
 * `id` joins the drawn group before as its last tab (the first one: the group after). A group left empty goes and
 * its share goes to the group joined.
 */
export function joinAsTab(
  groups: readonly ViewGroup[],
  id: string,
  drawnAt: readonly number[],
): readonly ViewGroup[] {
  const at = groupOf(groups, id);
  const before = neighbor(groups, id, -1, drawnAt);
  const target = before >= 0 ? before : neighbor(groups, id, 1, drawnAt);
  const from = groups[at];
  if (target < 0 || from === undefined) return groups;
  const rest = from.views.filter((v) => v !== id);
  const next = groups.map((g, i) => {
    if (i === target) {
      return { views: [...g.views, id], share: g.share + (rest.length === 0 ? from.share : 0) };
    }
    if (i === at) return { views: rest, share: g.share };
    return g;
  });
  return normalize(next.filter((g) => g.views.length > 0));
}

/** Show before | after can act: `id` shares its group, and there is room for one more group. */
export function canSplit(groups: readonly ViewGroup[], id: string): boolean {
  const group = groups[groupOf(groups, id)];
  return group !== undefined && group.views.length > 1 && groups.length < MAX_GROUPS;
}

/** `id` leaves its tab group for a new group right before | after it; each gets half of the old share. */
export function splitOut(
  groups: readonly ViewGroup[],
  id: string,
  where: "before" | "after",
): readonly ViewGroup[] {
  const at = groupOf(groups, id);
  const group = groups[at];
  if (group === undefined || !canSplit(groups, id)) return groups;
  const half = group.share / 2;
  const alone = { views: [id], share: half };
  const rest = { views: group.views.filter((v) => v !== id), share: half };
  const next = [...groups];
  next.splice(at, 1, ...(where === "before" ? [alone, rest] : [rest, alone]));
  return next;
}

/** Two groups share their room anew: `before` takes `ratio` (0..1) of what both had together. */
export function resizePair(
  groups: readonly ViewGroup[],
  before: number,
  after: number,
  ratio: number,
): readonly ViewGroup[] {
  const a = groups[before];
  const b = groups[after];
  if (a === undefined || b === undefined || !Number.isFinite(ratio)) return groups;
  const both = a.share + b.share;
  const share = Math.min(1, Math.max(0, ratio)) * both;
  return groups.map((g, i) =>
    i === before
      ? { views: g.views, share }
      : i === after
        ? { views: g.views, share: both - share }
        : g,
  );
}

/** The groups once `id` left its own: a group left empty goes, and the shares fill the region again. */
function without(groups: readonly ViewGroup[], id: string): readonly ViewGroup[] {
  const at = groupOf(groups, id);
  const group = groups[at];
  if (group === undefined) return groups;
  if (group.views.length === 1) return normalize(groups.filter((_, i) => i !== at));
  return groups.map((g, i) =>
    i === at ? { views: g.views.filter((v) => v !== id), share: g.share } : g,
  );
}

/** `id` out of the region that lists it; `gone` = the place of the group that went with it in that region. */
function lift(
  arr: Arrangement,
  id: string,
): { rest: Arrangement; from: Region | null; gone: number } {
  const from = regionOf(arr, id);
  if (from === null) return { rest: arr, from, gone: -1 };
  const at = groupOf(arr[from], id);
  const rest = withRegion(arr, from, without(arr[from], id));
  return { rest, from, gone: rest[from].length < arr[from].length ? at : -1 };
}

/**
 * `id` becomes the tab `index` of the group `group` of `region` (both as places of `arr`, before the move): it leaves
 * its own group (`without`), the target keeps its share. Inside its own group the tab changes place. No such group |
 * already that tab: nothing changes.
 */
export function intoGroup(
  arr: Arrangement,
  id: string,
  region: Region,
  group: number,
  index: number,
): Arrangement {
  const target = arr[region][group];
  if (target === undefined) return arr;
  const own = target.views.indexOf(id);
  const tabs = target.views.filter((v) => v !== id);
  const place = Math.min(tabs.length, Math.max(0, own >= 0 && index > own ? index - 1 : index));
  if (own === place) return arr;
  const { rest, from, gone } = lift(arr, id);
  // a group that went before the target moved it one place up
  const at = from === region && gone >= 0 && gone < group ? group - 1 : group;
  return withRegion(
    rest,
    region,
    rest[region].map((g, i) =>
      i === at ? { views: [...tabs.slice(0, place), id, ...tabs.slice(place)], share: g.share } : g,
    ),
  );
}

/**
 * One more group fits in `region`. For the view `id` (the one that would become that group): also in a full region
 * where it sits alone in a group, which goes with it (its group changes place, no group is added).
 */
export function canAddGroup(arr: Arrangement, region: Region, id?: string): boolean {
  const groups = arr[region];
  if (groups.length < MAX_GROUPS) return true;
  return id !== undefined && groups[groupOf(groups, id)]?.views.length === 1;
}

/**
 * `id` becomes a group of its own at the place `at` of `region` (a place of `arr`, before the move): it takes half of
 * the share of the larger group beside it (an empty region: the whole). No room for a group (`canAddGroup`) | a group
 * already at that place: nothing changes.
 */
export function asGroup(arr: Arrangement, id: string, region: Region, at: number): Arrangement {
  if (!canAddGroup(arr, region, id)) return arr;
  const { rest, from, gone } = lift(arr, id);
  const own = from === region ? gone : -1;
  if (own >= 0 && (at === own || at === own + 1)) return arr;
  const groups = rest[region];
  const place = Math.min(groups.length, Math.max(0, own >= 0 && own < at ? at - 1 : at));
  const before = groups[place - 1];
  const after = groups[place];
  const giver =
    before === undefined || (after !== undefined && after.share > before.share) ? after : before;
  if (giver === undefined) return withRegion(rest, region, [{ views: [id], share: 1 }]);
  const half = giver.share / 2;
  const next = groups.map((g) => (g === giver ? { views: g.views, share: half } : g));
  next.splice(place, 0, { views: [id], share: half });
  return withRegion(rest, region, next);
}

/** `id` moves to `region`: the last tab of its last group (an empty region: its first group). */
export function moveTo(arr: Arrangement, id: string, region: Region): Arrangement {
  const groups = arr[region];
  const last = groups[groups.length - 1];
  return last === undefined
    ? asGroup(arr, id, region, 0)
    : intoGroup(arr, id, region, groups.length - 1, last.views.length);
}

/**
 * Sizes (px) for drawn shares in `total` px, each >= `min`; `null` when even the minimums do not fit (the region
 * then draws its groups as one tab group).
 */
export function fitSizes(total: number, shares: readonly number[], min: number): number[] | null {
  if (shares.length === 0) return [];
  if (!Number.isFinite(total) || total < shares.length * min) return null;
  const fixed = new Set<number>();
  let sizes = shares.map((s) => s * total);
  for (;;) {
    const room = total - fixed.size * min;
    const free = shares.reduce((sum, s, i) => (fixed.has(i) ? sum : sum + s), 0);
    sizes = shares.map((s, i) => (fixed.has(i) ? min : free > 0 ? (s / free) * room : 0));
    const under = sizes.findIndex((h, i) => !fixed.has(i) && h < min);
    if (under < 0) break;
    fixed.add(under);
  }
  const whole = sizes.map((h) => Math.floor(h));
  const rest = total - whole.reduce((sum, h) => sum + h, 0);
  whole[whole.length - 1] = (whole[whole.length - 1] ?? 0) + rest;
  return whole;
}

/**
 * The drawn groups do not fit `size` px along the stack, each at `MIN_GROUP_PX`: the region draws them as one tab
 * group (the arrangement stays as saved). The dividers take their room; so do the tab strips of a stack that runs
 * down (`strip`: the group draws one; by default a group of several views). A size not measured yet (0) folds nothing.
 */
export function tooShort(
  size: number,
  groups: readonly DrawnGroup[],
  axis: Axis,
  strip: (group: DrawnGroup) => boolean = (group) => group.views.length > 1,
): boolean {
  if (groups.length < 2 || !(size > 0)) return false;
  const strips = axis === "vertical" ? groups.filter(strip).length : 0;
  const room = size - (groups.length - 1) * DIVIDER_PX - strips * STRIP_PX;
  const shares = groups.map((g) => g.share);
  return fitSizes(room, shares, MIN_GROUP_PX) === null;
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
