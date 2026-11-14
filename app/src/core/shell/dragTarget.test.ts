// Where a dragged view would land, pure: the one target under the pointer (a place among the tabs of a strip, a half
// of a view's body, the edge of a region that is not drawn) or none, the move a target makes and the words said after.
import { describe, expect, it } from "vitest";
import {
  asGroup,
  FILES,
  intoGroup,
  LIVE_VIEW,
  moveTo,
  OUTLINE,
  SEARCH,
  type Arrangement,
  type Region,
  type ViewGroup,
} from "./arrangement";
import {
  dragTarget,
  dropOn,
  EDGE_PX,
  saidFor,
  type Box,
  type DragScene,
  type DragTarget,
  type GroupBox,
} from "./dragTarget";

const group = (share: number, ...ids: string[]): ViewGroup => ({ views: ids, share });
const box = (left: number, top: number, right: number, bottom: number): Box => ({
  left,
  top,
  right,
  bottom,
});
const placed = (arr: Arrangement): Record<Region, string[][]> => ({
  left: arr.left.map((g) => [...g.views]),
  right: arr.right.map((g) => [...g.views]),
  bottom: arr.bottom.map((g) => [...g.views]),
});

// a window of 1000 x 700: the top bar (42), the left region (260 wide), the page, the right region (280 wide)
const BODY = box(0, 42, 1000, 676);
const CENTER = box(260, 42, 720, 676);
/** Three tabs at the left, over one body. */
const TABS = [
  { id: FILES, box: box(8, 50, 68, 80) },
  { id: SEARCH, box: box(70, 50, 140, 80) },
  { id: "c", box: box(142, 50, 200, 80) },
];
const LEFT: GroupBox = {
  at: 0,
  strip: { box: box(0, 42, 260, 80), tabs: TABS },
  body: box(0, 80, 260, 676),
};
/** Two views with a header of their own at the right, one above the other (a divider between). */
const RIGHT: GroupBox[] = [
  { at: 0, strip: null, body: box(720, 42, 1000, 356) },
  { at: 1, strip: null, body: box(720, 363, 1000, 676) },
];
const arr: Arrangement = {
  v: 2,
  left: [group(1, FILES, SEARCH, "c")],
  right: [group(0.5, OUTLINE), group(0.5, LIVE_VIEW)],
  bottom: [],
};
const scene: DragScene = {
  regions: [
    { region: "left", folded: false, groups: [LEFT] },
    { region: "right", folded: false, groups: RIGHT },
  ],
  body: BODY,
  center: CENTER,
};

/** A target without where it is drawn. */
const bare = (target: DragTarget | null): Record<string, unknown> | null => {
  if (target === null) return null;
  const { mark: _mark, ...rest } = target;
  return rest;
};

describe("over a tab strip", () => {
  it("the place among the tabs the pointer is nearest: at the start, between two tabs, at the end", () => {
    const at = (x: number): DragTarget | null => dragTarget({ x, y: 60 }, scene, OUTLINE, arr);
    expect(bare(at(5))).toEqual({ kind: "tab", region: "left", group: 0, index: 0 });
    expect(bare(at(60))).toEqual({ kind: "tab", region: "left", group: 0, index: 1 });
    expect(bare(at(80))).toEqual({ kind: "tab", region: "left", group: 0, index: 1 });
    expect(bare(at(120))).toEqual({ kind: "tab", region: "left", group: 0, index: 2 });
    // past the last tab, as far as the strip goes (its options button too)
    expect(bare(at(250))).toEqual({ kind: "tab", region: "left", group: 0, index: 3 });
  });

  it("is shown as a line 2 px wide between the tabs, as high as they are", () => {
    const line = (x: number): Box | undefined =>
      dragTarget({ x, y: 60 }, scene, OUTLINE, arr)?.mark;
    expect(line(5)).toEqual(box(7, 50, 9, 80));
    expect(line(80)).toEqual(box(68, 50, 70, 80));
    expect(line(250)).toEqual(box(199, 50, 201, 80));
  });

  it("the place the dragged tab already has is no target; another place of its own strip is", () => {
    const at = (x: number): DragTarget | null => dragTarget({ x, y: 60 }, scene, SEARCH, arr);
    // before | after itself
    expect(at(80)).toBeNull();
    expect(at(120)).toBeNull();
    expect(bare(at(5))).toEqual({ kind: "tab", region: "left", group: 0, index: 0 });
    expect(bare(at(250))).toEqual({ kind: "tab", region: "left", group: 0, index: 3 });
    // a view alone under a strip, over that strip
    const alone: Arrangement = { ...arr, left: [group(1, FILES)] };
    const one: DragScene = {
      ...scene,
      regions: [
        {
          region: "left",
          folded: false,
          groups: [{ ...LEFT, strip: { box: box(0, 42, 260, 80), tabs: TABS.slice(0, 1) } }],
        },
      ],
    };
    expect(dragTarget({ x: 5, y: 60 }, one, FILES, alone)).toBeNull();
    expect(dragTarget({ x: 200, y: 60 }, one, FILES, alone)).toBeNull();
  });

  it("counts places among all the views of the group, the ones not on screen too", () => {
    // `hidden` is not present: no tab; the tab after it still lands after it
    const withHidden: Arrangement = { ...arr, left: [group(1, FILES, "hidden", SEARCH, "c")] };
    const at = (x: number): DragTarget | null =>
      dragTarget({ x, y: 60 }, scene, OUTLINE, withHidden);
    expect(bare(at(80))).toEqual({ kind: "tab", region: "left", group: 0, index: 1 });
    expect(bare(at(120))).toEqual({ kind: "tab", region: "left", group: 0, index: 3 });
  });
});

describe("over a view's body", () => {
  it("at a side: a new group above | below, by the half the pointer is in", () => {
    const upper = dragTarget({ x: 800, y: 100 }, scene, "c", arr);
    expect(bare(upper)).toEqual({ kind: "group", region: "right", at: 0, side: "before" });
    expect(upper?.mark).toEqual(box(720, 42, 1000, 199));
    const lower = dragTarget({ x: 800, y: 300 }, scene, "c", arr);
    expect(bare(lower)).toEqual({ kind: "group", region: "right", at: 1, side: "after" });
    expect(lower?.mark).toEqual(box(720, 199, 1000, 356));
    // the body under the tabs of a group
    expect(bare(dragTarget({ x: 100, y: 600 }, scene, OUTLINE, arr))).toEqual({
      kind: "group",
      region: "left",
      at: 1,
      side: "after",
    });
  });

  it("at the bottom: a new group on the left | on the right", () => {
    const low: Arrangement = { ...arr, bottom: [group(1, "x")] };
    const under: DragScene = {
      ...scene,
      regions: [
        ...scene.regions,
        {
          region: "bottom",
          folded: false,
          groups: [{ at: 0, strip: null, body: box(260, 436, 720, 676) }],
        },
      ],
    };
    const left = dragTarget({ x: 300, y: 600 }, under, "c", low);
    expect(bare(left)).toEqual({ kind: "group", region: "bottom", at: 0, side: "before" });
    expect(left?.mark).toEqual(box(260, 436, 490, 676));
    const right = dragTarget({ x: 600, y: 450 }, under, "c", low);
    expect(bare(right)).toEqual({ kind: "group", region: "bottom", at: 1, side: "after" });
    expect(right?.mark).toEqual(box(490, 436, 720, 676));
  });

  it("its own place is no target: a view alone in its group over itself, or beside the group it is next to", () => {
    expect(dragTarget({ x: 800, y: 100 }, scene, OUTLINE, arr)).toBeNull();
    expect(dragTarget({ x: 800, y: 300 }, scene, OUTLINE, arr)).toBeNull();
    // right above the group below it: where it already is
    expect(dragTarget({ x: 800, y: 400 }, scene, OUTLINE, arr)).toBeNull();
    expect(bare(dragTarget({ x: 800, y: 600 }, scene, OUTLINE, arr))).toEqual({
      kind: "group",
      region: "right",
      at: 2,
      side: "after",
    });
    // a tab of a group over its own group's body: out into a group of its own
    expect(bare(dragTarget({ x: 100, y: 100 }, scene, SEARCH, arr))).toEqual({
      kind: "group",
      region: "left",
      at: 0,
      side: "before",
    });
  });

  it("a region with three groups takes no more; a view alone in one of them still changes place there", () => {
    const full: Arrangement = {
      v: 2,
      left: [group(1, FILES, SEARCH)],
      right: [group(0.4, "a"), group(0.3, "b", "t"), group(0.3, "c")],
      bottom: [],
    };
    const three: DragScene = {
      ...scene,
      regions: [
        { region: "left", folded: false, groups: [LEFT] },
        {
          region: "right",
          folded: false,
          groups: [
            { at: 0, strip: null, body: box(720, 42, 1000, 240) },
            { at: 1, strip: null, body: box(720, 247, 1000, 450) },
            { at: 2, strip: null, body: box(720, 457, 1000, 676) },
          ],
        },
      ],
    };
    // from another region; a tab of a group of that region
    expect(dragTarget({ x: 800, y: 100 }, three, FILES, full)).toBeNull();
    expect(dragTarget({ x: 800, y: 600 }, three, "t", full)).toBeNull();
    // alone in its group there: its group goes with it
    expect(bare(dragTarget({ x: 800, y: 600 }, three, "a", full))).toEqual({
      kind: "group",
      region: "right",
      at: 3,
      side: "after",
    });
    expect(dragTarget({ x: 800, y: 100 }, three, "a", full)).toBeNull();
    expect(dragTarget({ x: 800, y: 300 }, three, "a", full)).toBeNull();
    // the left region still has room
    expect(dragTarget({ x: 100, y: 300 }, three, "a", full)?.kind).toBe("group");
  });

  it("a region folded by a small window has only its tabs as targets", () => {
    const folded: DragScene = {
      ...scene,
      regions: [
        { region: "left", folded: false, groups: [LEFT] },
        {
          region: "right",
          folded: true,
          groups: [
            {
              at: 0,
              strip: {
                box: box(720, 42, 1000, 80),
                tabs: [
                  { id: OUTLINE, box: box(728, 50, 800, 80) },
                  { id: LIVE_VIEW, box: box(802, 50, 900, 80) },
                ],
              },
              body: box(720, 80, 1000, 676),
            },
          ],
        },
      ],
    };
    expect(dragTarget({ x: 800, y: 300 }, folded, FILES, arr)).toBeNull();
    // the tabs of two groups in one strip: the view joins the group of the tab before it
    expect(bare(dragTarget({ x: 801, y: 60 }, folded, FILES, arr))).toEqual({
      kind: "tab",
      region: "right",
      group: 0,
      index: 1,
    });
    expect(bare(dragTarget({ x: 950, y: 60 }, folded, FILES, arr))).toEqual({
      kind: "tab",
      region: "right",
      group: 1,
      index: 1,
    });
    expect(bare(dragTarget({ x: 722, y: 60 }, folded, FILES, arr))).toEqual({
      kind: "tab",
      region: "right",
      group: 0,
      index: 0,
    });
  });
});

describe("the edge of a region that is not drawn", () => {
  it("the bottom: a strip 40 px high at the foot of the center column", () => {
    const target = dragTarget({ x: 500, y: 650 }, scene, SEARCH, arr);
    expect(bare(target)).toEqual({ kind: "region", region: "bottom" });
    expect(target?.mark).toEqual(box(260, 676 - EDGE_PX, 720, 676));
    expect(dragTarget({ x: 500, y: 676 - EDGE_PX - 1 }, scene, SEARCH, arr)).toBeNull();
  });

  it("the left and the right: a strip 40 px wide at the edge of the window's body", () => {
    // a single file with its outline at the bottom: nothing at the sides
    const single: Arrangement = { v: 2, left: [], right: [], bottom: [group(1, OUTLINE)] };
    const only: DragScene = {
      regions: [
        {
          region: "bottom",
          folded: false,
          groups: [{ at: 0, strip: null, body: box(0, 436, 1000, 676) }],
        },
      ],
      body: BODY,
      center: BODY,
    };
    const left = dragTarget({ x: 10, y: 200 }, only, OUTLINE, single);
    expect(bare(left)).toEqual({ kind: "region", region: "left" });
    expect(left?.mark).toEqual(box(0, 42, EDGE_PX, 676));
    const right = dragTarget({ x: 990, y: 200 }, only, OUTLINE, single);
    expect(bare(right)).toEqual({ kind: "region", region: "right" });
    expect(right?.mark).toEqual(box(1000 - EDGE_PX, 42, 1000, 676));
    expect(dragTarget({ x: EDGE_PX, y: 200 }, only, OUTLINE, single)).toBeNull();
    // what a drawn region shows there comes first: its own body, not the edge beside it
    expect(dragTarget({ x: 10, y: 600 }, only, OUTLINE, single)).toBeNull();
  });

  it("a region that is drawn has no edge strip; a window without a frame has none at all", () => {
    // the right region is drawn: its edge is its views' (here the divider between them)
    expect(dragTarget({ x: 990, y: 359 }, scene, SEARCH, arr)).toBeNull();
    expect(dragTarget({ x: 10, y: 60 }, scene, OUTLINE, arr)?.kind).toBe("tab");
    const frameless = { ...scene, body: null, center: null };
    expect(dragTarget({ x: 500, y: 650 }, frameless, SEARCH, arr)).toBeNull();
  });
});

describe("anywhere else", () => {
  it("over the page, the top bar, the status bar, a divider: no target", () => {
    expect(dragTarget({ x: 500, y: 300 }, scene, SEARCH, arr)).toBeNull();
    expect(dragTarget({ x: 500, y: 10 }, scene, SEARCH, arr)).toBeNull();
    expect(dragTarget({ x: 500, y: 690 }, scene, SEARCH, arr)).toBeNull();
    expect(dragTarget({ x: 800, y: 360 }, scene, SEARCH, arr)).toBeNull();
  });
});

describe("a target", () => {
  const mark = box(0, 0, 0, 0);
  const tab: DragTarget = { kind: "tab", region: "right", group: 1, index: 0, mark };
  const below: DragTarget = { kind: "group", region: "right", at: 1, side: "after", mark };
  const bottom: DragTarget = { kind: "region", region: "bottom", mark };

  it("makes the move the menu makes: into a group at a tab, a group of its own, to another region", () => {
    expect(dropOn(arr, SEARCH, tab)).toEqual(intoGroup(arr, SEARCH, "right", 1, 0));
    expect(placed(dropOn(arr, SEARCH, tab))).toEqual({
      left: [[FILES, "c"]],
      right: [[OUTLINE], [SEARCH, LIVE_VIEW]],
      bottom: [],
    });
    expect(dropOn(arr, SEARCH, below)).toEqual(asGroup(arr, SEARCH, "right", 1));
    expect(placed(dropOn(arr, SEARCH, below)).right).toEqual([[OUTLINE], [SEARCH], [LIVE_VIEW]]);
    expect(dropOn(arr, SEARCH, bottom)).toEqual(moveTo(arr, SEARCH, "bottom"));
    expect(placed(dropOn(arr, SEARCH, bottom))).toEqual({
      left: [[FILES, "c"]],
      right: [[OUTLINE], [LIVE_VIEW]],
      bottom: [[SEARCH]],
    });
    // nothing to change: the same arrangement
    expect(dropOn(arr, LIVE_VIEW, below)).toBe(arr);
  });

  it("says what the menu says: the region a view moved to; inside its region, a tab | above | below | on a side", () => {
    // to another region, however it lands there
    expect(saidFor(arr, SEARCH, tab)).toBe("panel.moved.toRight");
    expect(saidFor(arr, SEARCH, below)).toBe("panel.moved.toRight");
    expect(saidFor(arr, SEARCH, bottom)).toBe("panel.moved.toBottom");
    expect(saidFor(arr, OUTLINE, { kind: "region", region: "left", mark })).toBe(
      "panel.moved.toLeft",
    );
    // inside its own region
    expect(saidFor(arr, OUTLINE, tab)).toBe("panel.moved.tab");
    expect(saidFor(arr, OUTLINE, below)).toBe("panel.moved.below");
    expect(saidFor(arr, LIVE_VIEW, { ...below, at: 0, side: "before" })).toBe("panel.moved.above");
    const low: Arrangement = { ...arr, bottom: [group(0.5, "x"), group(0.5, "y", "z")] };
    const beside: DragTarget = { kind: "group", region: "bottom", at: 0, side: "before", mark };
    expect(saidFor(low, "z", beside)).toBe("panel.moved.onLeft");
    expect(saidFor(low, "z", { ...beside, at: 2, side: "after" })).toBe("panel.moved.onRight");
  });
});
