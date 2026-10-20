// Where the drop badge and the drop questions sit: near the pointer, always inside the view. Pure geometry (CSS px).

export type Size = { width: number; height: number };
export type Spot = { x: number; y: number };

/** Space kept between a floating element and the view's edges. */
const MARGIN = 8;

/** The badge: above-right of the pointer (clear of the system's drag image and its tip below the cursor); flipped
 *  left | down when it would leave the view; never closer than 8 px to an edge. */
export function badgePlace(at: Spot, badge: Size, view: Size): Spot {
  let x = at.x + 14;
  let y = at.y - 30;
  if (x + badge.width > view.width - MARGIN) x = at.x - 14 - badge.width;
  if (y < MARGIN) y = at.y + 18;
  return { x: clamp(x, badge.width, view.width), y: clamp(y, badge.height, view.height) };
}

/** A question at the drop point: its top-left 8 px off the point, flipped to stay inside; a narrow view (< 360 px)
 *  centers it across at the point's height. */
export function popoverPlace(at: Spot, box: Size, view: Size): Spot {
  let x = view.width < 360 ? (view.width - box.width) / 2 : at.x + MARGIN;
  let y = at.y + MARGIN;
  if (x + box.width > view.width - MARGIN) x = at.x - MARGIN - box.width;
  if (y + box.height > view.height - MARGIN) y = at.y - MARGIN - box.height;
  return { x: clamp(x, box.width, view.width), y: clamp(y, box.height, view.height) };
}

/** `start` kept so `size` fits between the margins (a box larger than the view starts at the margin). */
function clamp(start: number, size: number, room: number): number {
  return Math.max(MARGIN, Math.min(start, room - MARGIN - size));
}
