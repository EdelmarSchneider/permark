// The arrangement of the window's views + a project's views, pure: what was saved made safe (saved data survives
// updates), what is drawn, the moves inside a region and between regions, sizes.
import { describe, expect, it } from "vitest";
import {
  asGroup,
  axisOf,
  canAddGroup,
  canJoin,
  canMove,
  canSplit,
  defaultArrangement,
  DIVIDER_PX,
  drawnGroups,
  FILES,
  fitSizes,
  groupOf,
  inRegion,
  intoGroup,
  isDefault,
  joinAsTab,
  LIVE_VIEW,
  MAX_GROUPS,
  MIN_GROUP_PX,
  moveTo,
  moveView,
  normalize,
  OUTLINE,
  parseArrangement,
  parseProjectViews,
  regionOf,
  REGIONS,
  resizePair,
  SEARCH,
  select,
  selectedIn,
  splitOut,
  STRIP_PX,
  tooShort,
  withRegion,
  withViews,
  type Arrangement,
  type Region,
  type ViewGroup,
} from "./arrangement";

const views = (groups: readonly ViewGroup[]): string[][] => groups.map((g) => [...g.views]);
const shares = (groups: readonly ViewGroup[]): number[] =>
  groups.map((g) => Math.round(g.share * 1000) / 1000);
const all = (): boolean => true;
/** The right column a value of 0.11 reads as, by the window's reader with nothing saved for the three regions
 *  (the region-level ops take one region's groups). */
const groupsOf = (raw: unknown): readonly ViewGroup[] => parseArrangement(undefined, raw).right;
/** Every region's views, for one look at a whole arrangement. */
const placed = (arr: Arrangement): Record<Region, string[][]> => ({
  left: views(arr.left),
  right: views(arr.right),
  bottom: views(arr.bottom),
});
const group = (share: number, ...ids: string[]): ViewGroup => ({ views: ids, share });

describe("the saved right column of 0.11 (v1)", () => {
  it("absent, damaged or of another version: the default (stacked, outline on top, half each)", () => {
    const def = defaultArrangement().right;
    expect(views(def)).toEqual([[OUTLINE], [LIVE_VIEW]]);
    expect(shares(def)).toEqual([0.5, 0.5]);
    for (const raw of [
      undefined,
      null,
      "x",
      3,
      [],
      {},
      { v: 2, groups: [] },
      { v: 1 },
      { v: 1, groups: "x" },
    ]) {
      expect(groupsOf(raw), JSON.stringify(raw)).toEqual(def);
    }
    // groups with nothing usable: the default too
    expect(groupsOf({ v: 1, groups: [{ views: [] }, { views: [3, ""] }, "x"] })).toEqual(def);
  });

  it("a valid v1 is kept, unknown ids included (a plugin that is off finds its place again)", () => {
    const raw = {
      v: 1,
      groups: [
        { views: ["live-feed.panel", "future.panel"], share: 0.3 },
        { views: ["outline"], share: 0.7 },
      ],
    };
    const parsed = groupsOf(raw);
    expect(views(parsed)).toEqual([["live-feed.panel", "future.panel"], ["outline"]]);
    expect(shares(parsed)).toEqual([0.3, 0.7]);
  });

  it("duplicates keep their first place; more than 3 groups join the third as tabs", () => {
    const parsed = groupsOf({
      v: 1,
      groups: [
        { views: ["a", "b"], share: 1 },
        { views: ["b", "c"], share: 1 },
        { views: ["d"], share: 1 },
        { views: ["e"], share: 1 },
        { views: ["a", "f"], share: 1 },
      ],
    });
    expect(parsed).toHaveLength(MAX_GROUPS);
    expect(views(parsed)).toEqual([["a", "b"], ["c"], ["d", "e", "f"]]);
  });

  it("shares 0, negative, not a number or summing to more than 1 are clamped and normalized", () => {
    const parsed = groupsOf({
      v: 1,
      groups: [
        { views: ["a"], share: 0 },
        { views: ["b"], share: -2 },
        { views: ["c"], share: "big" },
      ],
    });
    expect(shares(parsed)).toEqual([0.333, 0.333, 0.333]);
    const big = groupsOf({
      v: 1,
      groups: [
        { views: ["a"], share: 3 },
        { views: ["b"], share: 1 },
      ],
    });
    expect(shares(big)).toEqual([0.75, 0.25]);
  });
});

describe("the saved arrangement (three regions)", () => {
  const custom = {
    v: 1,
    groups: [
      { views: [LIVE_VIEW, "future.panel"], share: 0.3 },
      { views: [OUTLINE], share: 0.7 },
    ],
  };

  it("the default: Files | Search as tabs on the left, the outline over Live changes on the right, no bottom", () => {
    const def = defaultArrangement();
    expect(def.v).toBe(2);
    expect(placed(def)).toEqual({
      left: [[FILES, SEARCH]],
      right: [[OUTLINE], [LIVE_VIEW]],
      bottom: [],
    });
    expect(shares(def.left)).toEqual([1]);
    expect(shares(def.right)).toEqual([0.5, 0.5]);
    expect(REGIONS).toEqual(["left", "right", "bottom"]);
  });

  it("a readable value is kept: unknown ids, empty regions, each region's shares normalized on its own", () => {
    const parsed = parseArrangement(
      {
        v: 2,
        left: [
          { views: [FILES], share: 3 },
          { views: [SEARCH, "future.panel"], share: 1 },
        ],
        right: [],
        bottom: [
          { views: [OUTLINE], share: 0.2 },
          { views: [LIVE_VIEW], share: 0.2 },
        ],
      },
      custom,
    );
    expect(placed(parsed)).toEqual({
      left: [[FILES], [SEARCH, "future.panel"]],
      right: [],
      bottom: [[OUTLINE], [LIVE_VIEW]],
    });
    expect(shares(parsed.left)).toEqual([0.75, 0.25]);
    expect(shares(parsed.bottom)).toEqual([0.5, 0.5]);
    // nothing listed at all is readable too: every region stays empty
    expect(placed(parseArrangement({ v: 2, left: [], right: [], bottom: [] }, custom))).toEqual({
      left: [],
      right: [],
      bottom: [],
    });
  });

  it("a view listed twice keeps its first place across the regions (left, right, bottom)", () => {
    const parsed = parseArrangement(
      {
        v: 2,
        left: [{ views: ["a", "b", "a"], share: 1 }],
        right: [
          { views: ["b"], share: 1 },
          { views: ["c", "a"], share: 1 },
        ],
        bottom: [{ views: ["c", "d"], share: 1 }],
      },
      null,
    );
    expect(placed(parsed)).toEqual({ left: [["a", "b"]], right: [["c"]], bottom: [["d"]] });
    expect(shares(parsed.right)).toEqual([1]);
  });

  it("more than 3 groups in a region join its third as tabs; empty and unusable groups are dropped", () => {
    const parsed = parseArrangement(
      {
        v: 2,
        left: [{ views: [] }, "x", { views: [3, ""] }, { views: ["z"], share: 1 }],
        right: [],
        bottom: [
          { views: ["a"], share: 1 },
          { views: ["b"], share: 1 },
          { views: ["c"], share: 1 },
          { views: ["d"], share: 1 },
          { views: ["e", "f"], share: 1 },
        ],
      },
      null,
    );
    expect(parsed.bottom).toHaveLength(MAX_GROUPS);
    expect(placed(parsed)).toEqual({
      left: [["z"]],
      right: [],
      bottom: [["a"], ["b"], ["c", "d", "e", "f"]],
    });
    expect(shares(parsed.bottom)).toEqual([0.333, 0.333, 0.333]);
  });

  it("absent, damaged or of another version: the default left, the right column as 0.11 saved it, no bottom", () => {
    for (const raw of [
      undefined,
      null,
      "x",
      3,
      [],
      {},
      { v: 2 },
      { v: 2, left: [], right: "x", bottom: [] },
      { v: 2, left: [], right: [] },
      { v: 1, left: [], right: [], bottom: [] },
      { v: 3, left: [], right: [], bottom: [] },
      custom,
    ]) {
      const parsed = parseArrangement(raw, custom);
      expect(placed(parsed), JSON.stringify(raw)).toEqual({
        left: [[FILES, SEARCH]],
        right: [[LIVE_VIEW, "future.panel"], [OUTLINE]],
        bottom: [],
      });
      expect(shares(parsed.right), JSON.stringify(raw)).toEqual([0.3, 0.7]);
    }
    // neither key readable: the default
    for (const old of [undefined, null, "x", { v: 2, groups: [] }]) {
      expect(parseArrangement(null, old), JSON.stringify(old)).toEqual(defaultArrangement());
    }
  });
});

describe("what is drawn", () => {
  const known = [
    { id: FILES, region: "left" },
    { id: SEARCH, region: "left" },
    { id: OUTLINE, region: "right" },
    { id: LIVE_VIEW, region: "right" },
  ] as const;

  it("lists new views as tabs of the last group of their default region", () => {
    const def = defaultArrangement();
    expect(withViews(def, known)).toBe(def);
    const more = withViews(def, [
      ...known,
      { id: "unread.panel", region: "left" },
      { id: "comments.panel", region: "right" },
      { id: "tags.panel", region: "left" },
    ]);
    expect(placed(more)).toEqual({
      left: [[FILES, SEARCH, "unread.panel", "tags.panel"]],
      right: [[OUTLINE], [LIVE_VIEW, "comments.panel"]],
      bottom: [],
    });
    expect(shares(more.right)).toEqual([0.5, 0.5]);
    expect(withViews(more, known)).toBe(more);
  });

  it("an empty region gets its first group; a view listed in another region stays where the person put it", () => {
    const moved: Arrangement = {
      v: 2,
      left: [],
      right: [group(1, FILES, OUTLINE)],
      bottom: [],
    };
    const listed = withViews(moved, [...known, { id: "log.panel", region: "bottom" }]);
    expect(placed(listed)).toEqual({
      left: [[SEARCH]],
      right: [[FILES, OUTLINE, LIVE_VIEW]],
      bottom: [["log.panel"]],
    });
    expect(shares(listed.left)).toEqual([1]);
    expect(shares(listed.bottom)).toEqual([1]);
    // the same view named twice is listed once
    const twice = withViews(moved, [
      { id: "x", region: "bottom" },
      { id: "x", region: "left" },
    ]);
    expect(placed(twice)).toEqual({ left: [], right: [[FILES, OUTLINE]], bottom: [["x"]] });
  });

  it("draws only groups with a present view; their shares fill the region (auto-adjust)", () => {
    const groups = groupsOf({
      v: 1,
      groups: [
        { views: [OUTLINE], share: 0.6 },
        { views: [LIVE_VIEW], share: 0.4 },
      ],
    });
    const both = drawnGroups(groups, all);
    expect(both.map((g) => g.share)).toEqual([0.6, 0.4]);
    // a document without headings: Live takes the whole column; the saved shares stay
    const liveOnly = drawnGroups(groups, (id) => id !== OUTLINE);
    expect(liveOnly).toEqual([{ at: 1, views: [LIVE_VIEW], share: 1 }]);
    expect(shares(groups)).toEqual([0.6, 0.4]);
    expect(drawnGroups(groups, () => false)).toEqual([]);
    expect(drawnGroups([], all)).toEqual([]);
  });
});

describe("the moves inside a region", () => {
  const three = groupsOf({
    v: 1,
    groups: [
      { views: ["a"], share: 0.5 },
      { views: ["b"], share: 0.3 },
      { views: ["c"], share: 0.2 },
    ],
  });
  const drawnAt = [0, 1, 2];

  it("move before | after swaps with the drawn neighbor (shares travel); none at the ends", () => {
    expect(canMove(three, "a", -1, drawnAt)).toBe(false);
    expect(canMove(three, "c", 1, drawnAt)).toBe(false);
    const up = moveView(three, "b", -1, drawnAt);
    expect(views(up)).toEqual([["b"], ["a"], ["c"]]);
    expect(shares(up)).toEqual([0.3, 0.5, 0.2]);
    // a group not drawn (its view absent) is skipped
    const skip = moveView(three, "c", -1, [0, 2]);
    expect(views(skip)).toEqual([["c"], ["b"], ["a"]]);
    expect(moveView(three, "a", -1, drawnAt)).toBe(three);
  });

  it("show as tab joins the group before (the first one: the group after), its share goes along", () => {
    const joined = joinAsTab(three, "b", drawnAt);
    expect(views(joined)).toEqual([["a", "b"], ["c"]]);
    expect(shares(joined)).toEqual([0.8, 0.2]);
    const top = joinAsTab(three, "a", drawnAt);
    expect(views(top)).toEqual([["b", "a"], ["c"]]);
    expect(canJoin(three, "a", [0])).toBe(false);
    expect(joinAsTab(three, "a", [0])).toBe(three);
  });

  it("show before | after splits a tab group in two halves; never past 3 groups", () => {
    const tabs = groupsOf({ v: 1, groups: [{ views: [OUTLINE, LIVE_VIEW], share: 1 }] });
    expect(canSplit(tabs, LIVE_VIEW)).toBe(true);
    const after = splitOut(tabs, OUTLINE, "after");
    expect(views(after)).toEqual([[LIVE_VIEW], [OUTLINE]]);
    expect(shares(after)).toEqual([0.5, 0.5]);
    const before = splitOut(tabs, LIVE_VIEW, "before");
    expect(views(before)).toEqual([[LIVE_VIEW], [OUTLINE]]);
    expect(canSplit(three, "a")).toBe(false);
    const full = groupsOf({
      v: 1,
      groups: [{ views: ["a", "b"] }, { views: ["c"] }, { views: ["d"] }],
    });
    expect(canSplit(full, "a")).toBe(false);
    expect(splitOut(full, "a", "before")).toBe(full);
  });

  it("a divider shares two groups' room anew; the others keep theirs", () => {
    const resized = resizePair(three, 0, 1, 0.25);
    expect(shares(resized)).toEqual([0.2, 0.6, 0.2]);
    expect(resizePair(three, 0, 1, Number.NaN)).toBe(three);
    expect(shares(resizePair(three, 0, 1, 2))).toEqual([0.8, 0, 0.2]);
  });

  it("the same moves act in the left and the bottom region; the other regions stay as they are", () => {
    const arr: Arrangement = {
      v: 2,
      left: [group(0.5, FILES), group(0.5, SEARCH)],
      right: defaultArrangement().right,
      bottom: [group(1, "a", "b")],
    };
    expect(REGIONS.map((region) => axisOf(region))).toEqual(["vertical", "vertical", "horizontal"]);
    expect([regionOf(arr, SEARCH), regionOf(arr, "b"), regionOf(arr, "gone")]).toEqual([
      "left",
      "bottom",
      null,
    ]);
    expect(groupOf(arr.left, SEARCH)).toBe(1);

    const swapped = inRegion(arr, SEARCH, (groups) => moveView(groups, SEARCH, -1, [0, 1]));
    expect(placed(swapped)).toEqual({
      left: [[SEARCH], [FILES]],
      right: [[OUTLINE], [LIVE_VIEW]],
      bottom: [["a", "b"]],
    });
    expect(swapped.right).toBe(arr.right);
    expect(swapped.bottom).toBe(arr.bottom);

    const tabbed = inRegion(arr, FILES, (groups) => joinAsTab(groups, FILES, [0, 1]));
    expect(views(tabbed.left)).toEqual([[SEARCH, FILES]]);
    expect(shares(tabbed.left)).toEqual([1]);

    const beside = inRegion(arr, "b", (groups) => splitOut(groups, "b", "before"));
    expect(views(beside.bottom)).toEqual([["b"], ["a"]]);
    expect(shares(beside.bottom)).toEqual([0.5, 0.5]);
    expect(beside.left).toBe(arr.left);

    const wider = withRegion(beside, "bottom", resizePair(beside.bottom, 0, 1, 0.7));
    expect(shares(wider.bottom)).toEqual([0.7, 0.3]);

    // nothing changed -> the same object
    expect(inRegion(arr, FILES, (groups) => moveView(groups, FILES, -1, [0, 1]))).toBe(arr);
    expect(inRegion(arr, "gone", () => [])).toBe(arr);
    expect(withRegion(arr, "left", arr.left)).toBe(arr);
  });
});

describe("the default arrangement, recognized", () => {
  /** The views a window knows: the core's, a plugin's sidebar tab, a plugin's right panel. */
  const known = [
    { id: FILES, region: "left" },
    { id: SEARCH, region: "left" },
    { id: "unread.panel", region: "left" },
    { id: OUTLINE, region: "right" },
    { id: LIVE_VIEW, region: "right" },
    { id: "review.panel", region: "right" },
  ] as const;

  it("is the default itself, whether or not it lists the views known now", () => {
    expect(isDefault(defaultArrangement(), known)).toBe(true);
    expect(isDefault(withViews(defaultArrangement(), known), known)).toBe(true);
    expect(isDefault(defaultArrangement(), [])).toBe(true);
    // a share off by a rounding only
    const def = defaultArrangement();
    const nearly = { ...def, right: [group(0.5 + 1e-9, OUTLINE), group(0.5 - 1e-9, LIVE_VIEW)] };
    expect(isDefault(nearly, known)).toBe(true);
  });

  it("is not once a view changed place, order, group or region, or a share changed", () => {
    const def = withViews(defaultArrangement(), known);
    const changed: Arrangement[] = [
      moveTo(def, SEARCH, "right"),
      moveTo(def, OUTLINE, "bottom"),
      moveTo(def, "review.panel", "left"),
      withRegion(def, "left", [group(1, SEARCH, FILES, "unread.panel")]),
      inRegion(def, OUTLINE, (groups) => moveView(groups, OUTLINE, 1, [0, 1])),
      inRegion(def, OUTLINE, (groups) => joinAsTab(groups, OUTLINE, [0, 1])),
      inRegion(def, SEARCH, (groups) => splitOut(groups, SEARCH, "after")),
      inRegion(def, OUTLINE, (groups) => resizePair(groups, 0, 1, 0.6)),
      // a view nobody offers now, listed where the default has none
      { ...def, bottom: [group(1, "gone.panel")] },
    ];
    changed.forEach((arr, i) => expect(isDefault(arr, known), `case ${i}`).toBe(false));
  });
});

describe("the moves between regions", () => {
  const arr: Arrangement = {
    v: 2,
    left: [group(1, FILES, SEARCH)],
    right: [group(0.6, OUTLINE), group(0.4, LIVE_VIEW, "c")],
    bottom: [],
  };

  it("move to a region: the last tab of its last group; an empty region gets its first group", () => {
    const left = moveTo(arr, "c", "left");
    expect(placed(left)).toEqual({
      left: [[FILES, SEARCH, "c"]],
      right: [[OUTLINE], [LIVE_VIEW]],
      bottom: [],
    });
    expect(shares(left.left)).toEqual([1]);
    expect(shares(left.right)).toEqual([0.6, 0.4]);

    const down = moveTo(arr, FILES, "bottom");
    expect(placed(down)).toEqual({
      left: [[SEARCH]],
      right: [[OUTLINE], [LIVE_VIEW, "c"]],
      bottom: [[FILES]],
    });
    expect(shares(down.bottom)).toEqual([1]);
    expect(down.right).toBe(arr.right);
  });

  it("a group left empty goes: its region's shares fill the room again", () => {
    const moved = moveTo(arr, OUTLINE, "left");
    expect(placed(moved)).toEqual({
      left: [[FILES, SEARCH, OUTLINE]],
      right: [[LIVE_VIEW, "c"]],
      bottom: [],
    });
    expect(shares(moved.right)).toEqual([1]);
    // the last view of a region leaves: the region is empty
    const empty = moveTo(moveTo(arr, FILES, "right"), SEARCH, "right");
    expect(placed(empty)).toEqual({
      left: [],
      right: [[OUTLINE], [LIVE_VIEW, "c", FILES, SEARCH]],
      bottom: [],
    });
  });

  it("into a given group at a given tab; places are those of the arrangement before the move", () => {
    expect(views(intoGroup(arr, "c", "left", 0, 1).left)).toEqual([[FILES, "c", SEARCH]]);
    expect(views(intoGroup(arr, "c", "left", 0, 0).left)).toEqual([["c", FILES, SEARCH]]);
    expect(views(intoGroup(arr, "c", "left", 0, 99).left)).toEqual([[FILES, SEARCH, "c"]]);
    // its own region: the group before the target goes, the target is still the one meant
    const joined = intoGroup(arr, OUTLINE, "right", 1, 1);
    expect(views(joined.right)).toEqual([[LIVE_VIEW, OUTLINE, "c"]]);
    expect(shares(joined.right)).toEqual([1]);
    // its own group: the tab changes place
    expect(views(intoGroup(arr, LIVE_VIEW, "right", 1, 2).right)).toEqual([
      [OUTLINE],
      ["c", LIVE_VIEW],
    ]);
    expect(views(intoGroup(arr, "c", "right", 1, 0).right)).toEqual([[OUTLINE], ["c", LIVE_VIEW]]);
  });

  it("as a group of its own: half of a neighbor's share; the whole of an empty region", () => {
    const first = asGroup(arr, "c", "left", 0);
    expect(placed(first)).toEqual({
      left: [["c"], [FILES, SEARCH]],
      right: [[OUTLINE], [LIVE_VIEW]],
      bottom: [],
    });
    expect(shares(first.left)).toEqual([0.5, 0.5]);
    expect(views(asGroup(arr, "c", "left", 1).left)).toEqual([[FILES, SEARCH], ["c"]]);
    expect(views(asGroup(arr, "c", "left", 99).left)).toEqual([[FILES, SEARCH], ["c"]]);
    // between two groups: the larger neighbor gives half of its share
    const between = asGroup(arr, FILES, "right", 1);
    expect(views(between.right)).toEqual([[OUTLINE], [FILES], [LIVE_VIEW, "c"]]);
    expect(shares(between.right)).toEqual([0.3, 0.3, 0.4]);
    expect(views(between.left)).toEqual([[SEARCH]]);
    const alone = asGroup(arr, "c", "bottom", 0);
    expect(views(alone.bottom)).toEqual([["c"]]);
    expect(shares(alone.bottom)).toEqual([1]);
    // inside its own region: out of a tab group, or a group changing place
    expect(views(asGroup(arr, "c", "right", 0).right)).toEqual([["c"], [OUTLINE], [LIVE_VIEW]]);
    const last = asGroup(arr, OUTLINE, "right", 2);
    expect(views(last.right)).toEqual([[LIVE_VIEW, "c"], [OUTLINE]]);
    expect(shares(last.right)).toEqual([0.5, 0.5]);
  });

  it("never past 3 groups in a region", () => {
    const full: Arrangement = {
      v: 2,
      left: [group(1, FILES, SEARCH)],
      right: [group(0.4, "a"), group(0.3, "b"), group(0.3, "c")],
      bottom: [],
    };
    expect(canAddGroup(full, "right")).toBe(false);
    expect(canAddGroup(full, "left")).toBe(true);
    expect(canAddGroup(full, "bottom")).toBe(true);
    expect(asGroup(full, FILES, "right", 0)).toBe(full);
    // a move as a tab still works there
    expect(views(moveTo(full, FILES, "right").right)).toEqual([["a"], ["b"], ["c", FILES]]);
  });

  it("a view alone in a group of a full region still changes place there: its group goes with it", () => {
    const full: Arrangement = {
      v: 2,
      left: [group(1, FILES, SEARCH)],
      right: [group(0.4, "a"), group(0.3, "b", "t"), group(0.3, "c")],
      bottom: [],
    };
    expect(canAddGroup(full, "right", "a")).toBe(true);
    expect(canAddGroup(full, "right", "c")).toBe(true);
    // a tab of a group there would make a fourth; so would a view of another region
    expect(canAddGroup(full, "right", "t")).toBe(false);
    expect(canAddGroup(full, "right", FILES)).toBe(false);
    expect(canAddGroup(full, "left", "a")).toBe(true);
    const last = asGroup(full, "a", "right", 3);
    expect(views(last.right)).toEqual([["b", "t"], ["c"], ["a"]]);
    expect(shares(last.right).reduce((sum, share) => sum + share, 0)).toBeCloseTo(1);
    expect(views(asGroup(full, "c", "right", 0).right)).toEqual([["c"], ["a"], ["b", "t"]]);
    expect(views(asGroup(full, "a", "right", 2).right)).toEqual([["b", "t"], ["a"], ["c"]]);
    // still no fourth group, and its own place changes nothing
    expect(asGroup(full, "t", "right", 0)).toBe(full);
    expect(asGroup(full, "a", "right", 0)).toBe(full);
    expect(asGroup(full, "a", "right", 1)).toBe(full);
  });

  it("a move that changes nothing returns the same object", () => {
    // already the last tab of the last group; alone in the only group
    expect(moveTo(arr, "c", "right")).toBe(arr);
    expect(moveTo(arr, SEARCH, "left")).toBe(arr);
    const single = moveTo(arr, FILES, "bottom");
    expect(moveTo(single, FILES, "bottom")).toBe(single);
    // the tab it already is; a group that is not there
    expect(intoGroup(arr, FILES, "left", 0, 0)).toBe(arr);
    expect(intoGroup(arr, FILES, "left", 0, 1)).toBe(arr);
    expect(intoGroup(arr, FILES, "right", 5, 0)).toBe(arr);
    expect(intoGroup(arr, FILES, "bottom", 0, 0)).toBe(arr);
    // a group already at that place (before | after itself)
    expect(asGroup(arr, OUTLINE, "right", 0)).toBe(arr);
    expect(asGroup(arr, OUTLINE, "right", 1)).toBe(arr);
  });
});

describe("sizes and tabs", () => {
  it("fit the region with each group >= its minimum, or say it is too short", () => {
    expect(fitSizes(600, [0.5, 0.5], 120)).toEqual([300, 300]);
    expect(fitSizes(400, [0.9, 0.1], 120)).toEqual([280, 120]);
    expect(fitSizes(239, [0.5, 0.5], 120)).toBeNull();
    expect(fitSizes(500, [], 120)).toEqual([]);
    const three = fitSizes(401, [0.2, 0.3, 0.5], 120) ?? [];
    expect(three.reduce((a, b) => a + b, 0)).toBe(401);
    expect(Math.min(...three)).toBeGreaterThanOrEqual(120);
  });

  it("groups that do not fit along the stack fold into tabs: 210 px each, the dividers, the strips of a stack that runs down", () => {
    const two = drawnGroups([group(0.5, "a"), group(0.5, "b")], all);
    const need = 2 * MIN_GROUP_PX + DIVIDER_PX;
    expect(tooShort(need, two, "vertical")).toBe(false);
    expect(tooShort(need - 1, two, "vertical")).toBe(true);
    // a tab group's strip takes room of a stack that runs down, none of groups side by side
    const tabbed = drawnGroups([group(0.5, "a", "c"), group(0.5, "b")], all);
    expect(tooShort(need + STRIP_PX - 1, tabbed, "vertical")).toBe(true);
    expect(tooShort(need + STRIP_PX, tabbed, "vertical")).toBe(false);
    expect(tooShort(need, tabbed, "horizontal")).toBe(false);
    expect(tooShort(need - 1, tabbed, "horizontal")).toBe(true);
    // a group of one view that draws a strip all the same (a view without a header of its own) counts it too
    const strip = (g: { readonly views: readonly string[] }): boolean => g.views[0] === "a";
    expect(tooShort(need + STRIP_PX - 1, two, "vertical", strip)).toBe(true);
    expect(tooShort(need + STRIP_PX, two, "vertical", strip)).toBe(false);
    // one group always fits; a size not measured yet folds nothing
    expect(tooShort(10, drawnGroups([group(1, "a")], all), "vertical")).toBe(false);
    expect(tooShort(0, two, "vertical")).toBe(false);
    expect(tooShort(Number.NaN, two, "vertical")).toBe(false);
  });

  it("shares are clamped and sum 1; no group, no share", () => {
    expect(shares(normalize([group(3, "a"), group(1, "b")]))).toEqual([0.75, 0.25]);
    expect(normalize([])).toEqual([]);
  });

  it("the selected tab of a group: its choice among the present views, else the first", () => {
    expect(selectedIn([OUTLINE, LIVE_VIEW], [LIVE_VIEW])).toBe(LIVE_VIEW);
    expect(selectedIn([OUTLINE, LIVE_VIEW], ["gone"])).toBe(OUTLINE);
    expect(selectedIn([], [])).toBeNull();
    // one choice per group: selecting replaces the group's old one, keeps the others'
    expect(select(["x", OUTLINE], [OUTLINE, LIVE_VIEW], LIVE_VIEW)).toEqual([LIVE_VIEW, "x"]);
  });
});

describe("a project's saved views", () => {
  it("absent (0.6.x sessions), damaged or another version: nothing chosen yet", () => {
    for (const raw of [undefined, null, "x", { v: 2, open: { a: true } }, { open: { a: true } }]) {
      expect(parseProjectViews(raw), JSON.stringify(raw)).toEqual({ v: 1, open: {}, selected: [] });
    }
  });

  it("keeps boolean choices and selected ids only", () => {
    expect(
      parseProjectViews({
        v: 1,
        open: { [LIVE_VIEW]: false, odd: "yes", other: true },
        selected: [LIVE_VIEW, 3, "", LIVE_VIEW],
      }),
    ).toEqual({ v: 1, open: { [LIVE_VIEW]: false, other: true }, selected: [LIVE_VIEW] });
  });
});
