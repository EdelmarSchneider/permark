// The Outline while editing (docs/design/v0.2.md section 2): the reading view is hidden, so the outline is built from the
// editor's own headings, rebuilt 300 ms after typing stops, and a click scrolls the editor. Source and Visual both offer
// `OutlineSource`; this class is the part that is the same for them, with no editor or Svelte in it.

import type { OutlineApi, OutlineSource } from "../bridge";
import { buildOutline, MAX_HEADINGS, type OutlineModel } from "../outline";

/** Quiet time after the last change before the outline is rebuilt. */
export const REBUILD_DELAY_MS = 300;

/** Where the outline goes (the `outline` store). */
export type OutlineSink = {
  set(path: string, model: OutlineModel): void;
  setCurrent(index: number | null): void;
};

export class EditorOutline implements OutlineApi {
  private timer: ReturnType<typeof setTimeout> | null = null;
  private frame = 0;
  /** The headings of the last model, to tell a change in the headings from typing in a paragraph. */
  private signature: string | null = null;
  private kept = 0;
  /** The heading an outline click went to: it stays the current section until the person scrolls or types. */
  private pinned: number | null = null;
  private stop: (() => void) | null = null;
  private disposed = false;

  constructor(
    private readonly path: string,
    private readonly editor: OutlineSource & { focus(): void },
    private readonly sink: OutlineSink,
  ) {}

  /** The editor is ready: the outline shows its headings now, and follows its scrolling. */
  start(): void {
    this.stop = this.editor.onViewport((byUser) => this.viewport(byUser));
    this.rebuild();
  }

  /** The text (or the selection) changed: the outline is rebuilt once typing has stopped for a moment. */
  changed(): void {
    if (this.disposed) return;
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = setTimeout(() => this.rebuild(), REBUILD_DELAY_MS);
  }

  /** An outline click or Enter: the editor scrolls to the heading, which stays the current section. */
  goTo(index: number): void {
    this.editor.scrollToHeading(index);
    this.pinned = index;
    this.sink.setCurrent(index);
  }

  focusDoc(): void {
    this.editor.focus();
  }

  dispose(): void {
    this.disposed = true;
    if (this.timer !== null) clearTimeout(this.timer);
    if (this.frame !== 0) cancelAnimationFrame(this.frame);
    this.timer = null;
    this.frame = 0;
    this.stop?.();
    this.stop = null;
  }

  private rebuild(): void {
    this.timer = null;
    const headings = this.editor.headings();
    const signature = headings.map((h) => `${h.level}:${h.text}`).join("\n");
    if (signature !== this.signature) {
      this.signature = signature;
      this.pinned = null;
      const model = buildOutline(headings.map((h) => ({ ...h, changed: false })));
      this.kept = model.items.length;
      this.sink.set(this.path, model);
    }
    this.updateCurrent();
  }

  private viewport(byUser: boolean): void {
    if (byUser) this.pinned = null;
    if (this.frame !== 0) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.updateCurrent();
    });
  }

  private updateCurrent(): void {
    if (this.pinned !== null) {
      this.sink.setCurrent(this.pinned);
      return;
    }
    const at = this.editor.currentHeading();
    // a heading beyond the cut is not listed: the last listed one stands for it
    this.sink.setCurrent(
      at === null || this.kept === 0 ? null : Math.min(at, this.kept - 1, MAX_HEADINGS - 1),
    );
  }
}
