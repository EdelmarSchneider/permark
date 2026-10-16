// How an image is zoomed in the viewer: fitted to the pane (the default), or at a scale. Pure logic; `stores/media.svelte.ts`
// holds the state and `MediaView.svelte` draws it.

export type Size = { width: number; height: number };

/** Fitted to the pane, or at a scale (1 is the image's own pixels). */
export type ImageZoom = { mode: "fit" } | { mode: "scale"; scale: number };

export const FIT: ImageZoom = { mode: "fit" };

/** The scales Ctrl+= and Ctrl+- step through: from 5% to 1600%, the usual ones in between. */
export const ZOOM_STEPS: readonly number[] = [
  0.05, 0.1, 0.25, 0.33, 0.5, 0.67, 0.75, 1, 1.25, 1.5, 2, 3, 4, 6, 8, 16,
];

/** Below this a scale cannot be told from another, so one is "the same" as the other. */
const SAME = 1e-4;

/**
 * The scale that fits an image into a box. A picture that is smaller than the box is not blown up (fitting never goes
 * above 100%). An image with no size known yet fits at 100%.
 */
export function fitScale(natural: Size | null, box: Size): number {
  if (!natural || natural.width <= 0 || natural.height <= 0) return 1;
  if (box.width <= 0 || box.height <= 0) return 1;
  return Math.min(1, box.width / natural.width, box.height / natural.height);
}

/** The scale an image is drawn at. */
export function scaleOf(zoom: ImageZoom, fit: number): number {
  return zoom.mode === "fit" ? fit : zoom.scale;
}

/** Whether two scales are the same to the eye. */
function same(a: number, b: number): boolean {
  return Math.abs(a - b) < SAME;
}

/**
 * One step in (1) or out (-1) from what is on screen: the next preset above or below the current scale, or the same zoom
 * at the ends of the list. From a fitted image the step starts at the fit scale, so Ctrl+- on an image shown at 38%
 * goes to 33%, not to a preset far from what is seen.
 */
export function stepZoom(zoom: ImageZoom, direction: 1 | -1, fit: number): ImageZoom {
  const current = scaleOf(zoom, fit);
  const next =
    direction === 1
      ? ZOOM_STEPS.find((step) => step > current && !same(step, current))
      : [...ZOOM_STEPS].reverse().find((step) => step < current && !same(step, current));
  return next === undefined ? zoom : { mode: "scale", scale: next };
}

/**
 * The click on the image, and the button: from the fitted view to the image's own pixels, and back. At 100% already it
 * goes back to fit; from any other scale it goes to 100%.
 */
export function toggleActualSize(zoom: ImageZoom, fit: number): ImageZoom {
  if (zoom.mode === "fit") return same(fit, 1) ? zoom : { mode: "scale", scale: 1 };
  return same(zoom.scale, 1) ? FIT : { mode: "scale", scale: 1 };
}

/** The scale as the status bar writes it: "38%". */
export function percentLabel(scale: number): string {
  return `${Math.round(scale * 100)}%`;
}

/** The size an image is drawn at, in whole pixels (at least one). */
export function drawnSize(natural: Size, scale: number): Size {
  return {
    width: Math.max(1, Math.round(natural.width * scale)),
    height: Math.max(1, Math.round(natural.height * scale)),
  };
}

/** What the scrolling pane measures: the same numbers as the properties of a DOM element of those names. */
export type Viewport = {
  scrollLeft: number;
  scrollTop: number;
  clientWidth: number;
  clientHeight: number;
  scrollWidth: number;
  scrollHeight: number;
};

/** A place in the picture, as fractions (0 to 1) of its width and height. */
export type Point = { x: number; y: number };

/** Where the middle of the pane sits in the scrolled content. */
export function centreOf(view: Viewport): Point {
  return {
    x: (view.scrollLeft + view.clientWidth / 2) / Math.max(1, view.scrollWidth),
    y: (view.scrollTop + view.clientHeight / 2) / Math.max(1, view.scrollHeight),
  };
}

/**
 * The scroll position that puts `centre` back in the middle of the pane once the content has another size (`view` is
 * measured after the change). The browser clamps a scroll position the same way: never before the start or past the end.
 */
export function scrollToCentre(centre: Point, view: Viewport): { left: number; top: number } {
  const place = (at: number, content: number, pane: number): number =>
    Math.min(Math.max(0, at * content - pane / 2), Math.max(0, content - pane));
  return {
    left: place(centre.x, view.scrollWidth, view.clientWidth),
    top: place(centre.y, view.scrollHeight, view.clientHeight),
  };
}
