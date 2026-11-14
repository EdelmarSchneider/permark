// Where a dragged view would land: from the pointer and the boxes of what the window draws (the tab strips and view
// bodies of its regions, the frame whose edges stand for the regions that are not drawn) to ONE target, or none.
// Pure: nothing of the page is read here. A target also gives the move it makes (the ones the views' menu makes) and
// the words said once it is made.

import type { MessageKey } from "../i18n";
import {
  asGroup,
  axisOf,
  canAddGroup,
  groupOf,
  intoGroup,
  moveTo,
  regionOf,
  REGIONS,
  type Arrangement,
  type Region,
} from "./arrangement";
import { ACROSS, ALONG, AS_TAB } from "./moveWords";

/** The strip along an edge of the frame that stands for a region that is not drawn. */
export const EDGE_PX = 40;
/** The line that shows a place between two tabs. */
const LINE_PX = 2;

export type Point = { readonly x: number; readonly y: number };
/** A box of the window, in its own coordinates (as an element's bounding box). */
export type Box = {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
};

/** A tab strip as drawn: its box and its tabs, in the order they show. */
export type StripBox = {
  readonly box: Box;
  readonly tabs: readonly { readonly id: string; readonly box: Box }[];
};

/** A drawn group: its place among the groups of its region as arranged, its strip (`null`: it draws none), the body
 *  of the view it shows. */
export type GroupBox = { readonly at: number; readonly strip: StripBox | null; readonly body: Box };

/** A drawn region; `folded`: a small window shows its groups as one tab group. */
export type RegionBox = {
  readonly region: Region;
  readonly folded: boolean;
  readonly groups: readonly GroupBox[];
};

/**
 * What the window draws now. `regions`: the drawn ones only (a region missing here has no present view). `body`: the
 * frame between the top bar and the status bar; `center`: its column with the page (`null`: no frame on screen).
 */
export type DragScene = {
  readonly regions: readonly RegionBox[];
  readonly body: Box | null;
  readonly center: Box | null;
};

/**
 * Where a drop puts the view, and `mark`, where that is shown. `tab`: the tab `index` of the group `group` (the line
 * between two tabs). `group`: a group of its own at the place `at`, before | after the group under the pointer (that
 * half of its body). `region`: into a region that is not drawn (the strip at its edge). Places are those of the
 * arrangement before the move.
 */
export type DragTarget =
  | {
      readonly kind: "tab";
      readonly region: Region;
      readonly group: number;
      readonly index: number;
      readonly mark: Box;
    }
  | {
      readonly kind: "group";
      readonly region: Region;
      readonly at: number;
      readonly side: "before" | "after";
      readonly mark: Box;
    }
  | { readonly kind: "region"; readonly region: Region; readonly mark: Box };

function holds(box: Box, at: Point): boolean {
  return at.x >= box.left && at.x < box.right && at.y >= box.top && at.y < box.bottom;
}

/**
 * The place among the tabs of a strip the pointer is nearest. The view joins the group of the tab before that place,
 * right after it (the first place: before the first tab): a strip may show the tabs of several groups (a folded
 * region), and a group may list views that are not on screen.
 */
function onStrip(strip: StripBox, region: Region, at: Point, arr: Arrangement): DragTarget | null {
  const place = strip.tabs.filter((tab) => (tab.box.left + tab.box.right) / 2 < at.x).length;
  const before = strip.tabs[place - 1];
  const after = strip.tabs[place];
  const beside = before ?? after;
  if (beside === undefined) return null;
  const group = groupOf(arr[region], beside.id);
  const views = arr[region][group]?.views;
  if (views === undefined) return null;
  const x =
    before === undefined
      ? beside.box.left
      : after === undefined
        ? before.box.right
        : (before.box.right + after.box.left) / 2;
  return {
    kind: "tab",
    region,
    group,
    index: views.indexOf(beside.id) + (before === undefined ? 0 : 1),
    mark: {
      left: x - LINE_PX / 2,
      top: beside.box.top,
      right: x + LINE_PX / 2,
      bottom: beside.box.bottom,
    },
  };
}

/** The half of a group's body the pointer is in, along the way its region stacks: a group before | after it. */
function onBody(group: GroupBox, region: Region, at: Point): DragTarget {
  const { left, top, right, bottom } = group.body;
  const upright = axisOf(region) === "vertical";
  const middle = upright ? (top + bottom) / 2 : (left + right) / 2;
  const first = (upright ? at.y : at.x) < middle;
  const mark = upright
    ? { left, top: first ? top : middle, right, bottom: first ? middle : bottom }
    : { left: first ? left : middle, top, right: first ? middle : right, bottom };
  return {
    kind: "group",
    region,
    at: first ? group.at : group.at + 1,
    side: first ? "before" : "after",
    mark,
  };
}

/** The strip that stands for a region that is not drawn: the left | right edge of the frame's body, the foot of
 *  its center column (`null`: no frame). */
function edgeOf(region: Region, scene: DragScene): Box | null {
  const { body, center } = scene;
  switch (region) {
    case "left":
      return (
        body && { left: body.left, top: body.top, right: body.left + EDGE_PX, bottom: body.bottom }
      );
    case "right":
      return (
        body && {
          left: body.right - EDGE_PX,
          top: body.top,
          right: body.right,
          bottom: body.bottom,
        }
      );
    case "bottom":
      return (
        center && {
          left: center.left,
          top: center.bottom - EDGE_PX,
          right: center.right,
          bottom: center.bottom,
        }
      );
    default:
      return region satisfies never;
  }
}

/** What is under the pointer, whether or not a drop there would change anything. */
function under(at: Point, scene: DragScene, id: string, arr: Arrangement): DragTarget | null {
  for (const { region, folded, groups } of scene.regions) {
    for (const group of groups) {
      if (group.strip !== null && holds(group.strip.box, at)) {
        return onStrip(group.strip, region, at, arr);
      }
      if (holds(group.body, at)) {
        // a folded region cannot draw one more group, a full one cannot take it
        return folded || !canAddGroup(arr, region, id) ? null : onBody(group, region, at);
      }
    }
  }
  for (const region of REGIONS) {
    if (scene.regions.some((drawn) => drawn.region === region)) continue;
    const mark = edgeOf(region, scene);
    if (mark !== null && holds(mark, at)) return { kind: "region", region, mark };
  }
  return null;
}

/**
 * The one target for the view `id` dragged to `at` (`null`: none). Over a tab strip: a place among its tabs. Over a
 * view's body: a group of its own before | after that group, by the half the pointer is in; not in a region that
 * can take no more groups (unless the view is alone in a group there: its group changes place), nor in a folded
 * one. At the edge of the frame where a region is not drawn: into that region. Over the page | anything else, and
 * where a drop would change nothing (the place the view has): none.
 */
export function dragTarget(
  at: Point,
  scene: DragScene,
  id: string,
  arr: Arrangement,
): DragTarget | null {
  const target = under(at, scene, id, arr);
  return target === null || dropOn(arr, id, target) === arr ? null : target;
}

/** The arrangement once `id` is dropped on `target` (nothing to change: `arr` itself). */
export function dropOn(arr: Arrangement, id: string, target: DragTarget): Arrangement {
  switch (target.kind) {
    case "tab":
      return intoGroup(arr, id, target.region, target.group, target.index);
    case "group":
      return asGroup(arr, id, target.region, target.at);
    case "region":
      return moveTo(arr, id, target.region);
    default:
      return target satisfies never;
  }
}

/**
 * What is said once `id` is dropped on `target`, in the menu's words: to another region, that it moved there
 * (however it sits there); inside its own region, that it is a tab now | where its group went.
 */
export function saidFor(arr: Arrangement, id: string, target: DragTarget): MessageKey {
  if (target.kind === "region" || regionOf(arr, id) !== target.region) {
    return ACROSS[target.region].said;
  }
  switch (target.kind) {
    case "tab":
      return AS_TAB.said;
    case "group":
      return ALONG[axisOf(target.region)][target.side].said;
    default:
      return target satisfies never;
  }
}
