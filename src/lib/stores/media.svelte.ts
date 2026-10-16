// What the viewer of a picture, sound or video keeps for the file on show: how the picture is zoomed, the pane it is
// fitted into, and what the webview learned of the file once it had decoded it (its pixels, how long it plays). The
// keys, the status bar and the header read it; `MediaView.svelte` fills it in.

import {
  FIT,
  fitScale,
  percentLabel,
  scaleOf,
  stepZoom,
  toggleActualSize,
  type ImageZoom,
  type Size,
} from "../imageZoom";
import type { MediaFacts } from "../mediaInfo";

/** What the view provides: it makes a change of the zoom and keeps the middle of the picture in the middle of the pane. */
export type ZoomAnchor = (change: () => void) => void;

class MediaStore {
  /** The file on show. */
  path = $state<string | null>(null);
  zoom = $state.raw<ImageZoom>(FIT);
  /** The room the picture is fitted into. */
  box = $state.raw<Size>({ width: 0, height: 0 });
  /** What the webview learned of the file on show. */
  facts = $state.raw<MediaFacts>({});

  private anchor: ZoomAnchor | null = null;

  /** The picture's own pixels, once known. */
  natural = $derived<Size | null>(
    this.facts.width && this.facts.height
      ? { width: this.facts.width, height: this.facts.height }
      : null,
  );
  /** The scale at which the picture fits the pane. */
  fit = $derived(fitScale(this.natural, this.box));
  /** The scale the picture is drawn at. */
  scale = $derived(scaleOf(this.zoom, this.fit));
  isFit = $derived(this.zoom.mode === "fit");
  /** The scale as the status bar writes it. */
  label = $derived(percentLabel(this.scale));

  /**
   * A file is on show. A different one starts over (fitted, nothing known of it); the same one, read again because it
   * changed on disk, keeps its zoom, and what is known of it until it is decoded again.
   */
  show(path: string): void {
    if (path === this.path) return;
    this.path = path;
    this.zoom = FIT;
    this.facts = {};
  }

  /** Nothing of this kind is on show any more. */
  clear(): void {
    this.path = null;
    this.zoom = FIT;
    this.facts = {};
  }

  /** The webview decoded the file on show: its pixels, or how long it plays. Merged into what is known. */
  noteFacts(path: string, facts: MediaFacts): void {
    if (path !== this.path) return;
    this.facts = { ...this.facts, ...facts };
  }

  setBox(box: Size): void {
    if (box.width !== this.box.width || box.height !== this.box.height) this.box = box;
  }

  /** The view of the picture on screen lends the store its way to keep the middle in place; null when it goes away. */
  useAnchor(anchor: ZoomAnchor | null): void {
    this.anchor = anchor;
  }

  /** Ctrl+= and Ctrl+- (and the buttons, and Ctrl+wheel). */
  zoomBy(direction: 1 | -1): void {
    this.change(() => (this.zoom = stepZoom(this.zoom, direction, this.fit)));
  }

  /** Ctrl+0: back to fitting the pane. */
  reset(): void {
    this.change(() => (this.zoom = FIT));
  }

  /** The click on the picture and the button: fit, and 100%. */
  toggleActual(): void {
    this.change(() => (this.zoom = toggleActualSize(this.zoom, this.fit)));
  }

  /** Every change of the zoom goes through here, so the keys, the buttons and the wheel all keep the middle in the middle. */
  private change(apply: () => void): void {
    if (this.anchor) this.anchor(apply);
    else apply();
  }
}

export const media = new MediaStore();
