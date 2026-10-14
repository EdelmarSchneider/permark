// The open document's outline (pure model in ../outline.ts) as reactive state. DocView builds it from the
// rendered headings after every render and reports the section being read; the panel and the popover show it.

import { CollapseMemory, EMPTY_OUTLINE, type OutlineModel } from "../outline";

class OutlineState {
  /** The document the model belongs to. */
  path = $state<string | null>(null);
  model = $state.raw<OutlineModel>(EMPTY_OUTLINE);
  /** Collapse keys of the open document (see `CollapseMemory`). */
  collapsed = $state.raw<ReadonlySet<string>>(new Set());
  /** The heading of the section being read, as an index into `model.items`. */
  current = $state<number | null>(null);

  private memory = new CollapseMemory();

  /** A new outline for a document: a fresh open or a live update. Collapsed headings stay collapsed by text. */
  set(path: string, model: OutlineModel): void {
    if (path !== this.path) this.collapsed = this.memory.get(path);
    this.path = path;
    this.model = model;
    if (this.current !== null && this.current >= model.items.length) this.current = null;
  }

  /** No outline: no document, or one that is not Markdown. */
  clear(): void {
    this.path = null;
    this.model = EMPTY_OUTLINE;
    this.current = null;
    this.collapsed = new Set();
  }

  /** Another project was opened. */
  reset(): void {
    this.clear();
    this.memory.clear();
  }

  setCurrent(index: number | null): void {
    if (this.current !== index) this.current = index;
  }

  /** Collapses or expands one heading (flips it when `expand` is not given). */
  toggle(index: number, expand?: boolean): void {
    const item = this.model.items[index];
    if (!item || !item.hasChildren || this.path === null) return;
    const open = expand ?? this.collapsed.has(item.key);
    this.collapsed = this.memory.set(this.path, item.key, !open);
  }
}

export const outline = new OutlineState();
