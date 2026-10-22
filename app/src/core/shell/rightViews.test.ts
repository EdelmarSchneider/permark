// The right column's arrangement + a project's views, pure: what was saved made safe (saved data survives updates),
// what is drawn, the menu's moves, heights (shell.md §Frame, Right region; design-live-follow §14.7 #20).
import { describe, expect, it } from "vitest";
import {
  canJoin,
  canMove,
  canSplit,
  defaultRightViews,
  drawnGroups,
  fitHeights,
  joinAsTab,
  LIVE_VIEW,
  MAX_GROUPS,
  moveView,
  OUTLINE,
  parseProjectViews,
  parseRightViews,
  resizePair,
  select,
  selectedIn,
  splitOut,
  withViews,
  type RightViews,
} from "./rightViews";

const views = (layout: RightViews): string[][] => layout.groups.map((g) => [...g.views]);
const shares = (layout: RightViews): number[] =>
  layout.groups.map((g) => Math.round(g.share * 1000) / 1000);
const all = (): boolean => true;

describe("the saved arrangement", () => {
  it("absent, damaged or of another version: the default (stacked, outline on top, half each)", () => {
    const def = defaultRightViews();
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
      expect(parseRightViews(raw), JSON.stringify(raw)).toEqual(def);
    }
    // groups with nothing usable: the default too
    expect(parseRightViews({ v: 1, groups: [{ views: [] }, { views: [3, ""] }, "x"] })).toEqual(
      def,
    );
  });

  it("a valid v1 is kept, unknown ids included (a plugin that is off finds its place again)", () => {
    const raw = {
      v: 1,
      groups: [
        { views: ["live-feed.panel", "future.panel"], share: 0.3 },
        { views: ["outline"], share: 0.7 },
      ],
    };
    const parsed = parseRightViews(raw);
    expect(views(parsed)).toEqual([["live-feed.panel", "future.panel"], ["outline"]]);
    expect(shares(parsed)).toEqual([0.3, 0.7]);
  });

  it("duplicates keep their first place; more than 3 groups join the third as tabs", () => {
    const parsed = parseRightViews({
      v: 1,
      groups: [
        { views: ["a", "b"], share: 1 },
        { views: ["b", "c"], share: 1 },
        { views: ["d"], share: 1 },
        { views: ["e"], share: 1 },
        { views: ["a", "f"], share: 1 },
      ],
    });
    expect(parsed.groups).toHaveLength(MAX_GROUPS);
    expect(views(parsed)).toEqual([["a", "b"], ["c"], ["d", "e", "f"]]);
  });

  it("shares 0, negative, not a number or summing to more than 1 are clamped and normalized", () => {
    const parsed = parseRightViews({
      v: 1,
      groups: [
        { views: ["a"], share: 0 },
        { views: ["b"], share: -2 },
        { views: ["c"], share: "big" },
      ],
    });
    expect(shares(parsed)).toEqual([0.333, 0.333, 0.333]);
    const big = parseRightViews({
      v: 1,
      groups: [
        { views: ["a"], share: 3 },
        { views: ["b"], share: 1 },
      ],
    });
    expect(shares(big)).toEqual([0.75, 0.25]);
  });
});

describe("what is drawn", () => {
  it("lists new views as tabs of the last group", () => {
    const layout = withViews(defaultRightViews(), [OUTLINE, LIVE_VIEW, "comments.panel"]);
    expect(views(layout)).toEqual([[OUTLINE], [LIVE_VIEW, "comments.panel"]]);
    expect(withViews(layout, [OUTLINE])).toBe(layout);
  });

  it("draws only groups with a present view; their shares fill the column (auto-adjust)", () => {
    const layout = parseRightViews({
      v: 1,
      groups: [
        { views: [OUTLINE], share: 0.6 },
        { views: [LIVE_VIEW], share: 0.4 },
      ],
    });
    const both = drawnGroups(layout, all);
    expect(both.map((g) => g.share)).toEqual([0.6, 0.4]);
    // a document without headings: Live takes the whole column; the saved shares stay
    const liveOnly = drawnGroups(layout, (id) => id !== OUTLINE);
    expect(liveOnly).toEqual([{ at: 1, views: [LIVE_VIEW], share: 1 }]);
    expect(shares(layout)).toEqual([0.6, 0.4]);
    expect(drawnGroups(layout, () => false)).toEqual([]);
  });
});

describe("the menu's moves", () => {
  const three = parseRightViews({
    v: 1,
    groups: [
      { views: ["a"], share: 0.5 },
      { views: ["b"], share: 0.3 },
      { views: ["c"], share: 0.2 },
    ],
  });
  const drawnAt = [0, 1, 2];

  it("move up | down swaps with the drawn neighbor (shares travel); none at the ends", () => {
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

  it("show as tab joins the group above (the top one: below), its share goes along", () => {
    const joined = joinAsTab(three, "b", drawnAt);
    expect(views(joined)).toEqual([["a", "b"], ["c"]]);
    expect(shares(joined)).toEqual([0.8, 0.2]);
    const top = joinAsTab(three, "a", drawnAt);
    expect(views(top)).toEqual([["b", "a"], ["c"]]);
    expect(canJoin(three, "a", [0])).toBe(false);
  });

  it("show above | below splits a tab group in two halves; never past 3 groups", () => {
    const tabs = parseRightViews({ v: 1, groups: [{ views: [OUTLINE, LIVE_VIEW], share: 1 }] });
    expect(canSplit(tabs, LIVE_VIEW)).toBe(true);
    const below = splitOut(tabs, OUTLINE, "below");
    expect(views(below)).toEqual([[LIVE_VIEW], [OUTLINE]]);
    expect(shares(below)).toEqual([0.5, 0.5]);
    const above = splitOut(tabs, LIVE_VIEW, "above");
    expect(views(above)).toEqual([[LIVE_VIEW], [OUTLINE]]);
    expect(canSplit(three, "a")).toBe(false);
    const full = parseRightViews({
      v: 1,
      groups: [{ views: ["a", "b"] }, { views: ["c"] }, { views: ["d"] }],
    });
    expect(canSplit(full, "a")).toBe(false);
    expect(splitOut(full, "a", "above")).toBe(full);
  });

  it("a divider shares two groups' room anew; the others keep theirs", () => {
    const resized = resizePair(three, 0, 1, 0.25);
    expect(shares(resized)).toEqual([0.2, 0.6, 0.2]);
    expect(resizePair(three, 0, 1, Number.NaN)).toBe(three);
    expect(shares(resizePair(three, 0, 1, 2))).toEqual([0.8, 0, 0.2]);
  });
});

describe("heights and tabs", () => {
  it("fit the column with each group >= its minimum, or say it is too short", () => {
    expect(fitHeights(600, [0.5, 0.5], 120)).toEqual([300, 300]);
    expect(fitHeights(400, [0.9, 0.1], 120)).toEqual([280, 120]);
    expect(fitHeights(239, [0.5, 0.5], 120)).toBeNull();
    expect(fitHeights(500, [], 120)).toEqual([]);
    const three = fitHeights(401, [0.2, 0.3, 0.5], 120) ?? [];
    expect(three.reduce((a, b) => a + b, 0)).toBe(401);
    expect(Math.min(...three)).toBeGreaterThanOrEqual(120);
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
