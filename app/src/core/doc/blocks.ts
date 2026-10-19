// The blocks of a rendered Markdown document in the DOM, kept by id.
//
// A document arrives as blocks (content ids; HTML only for the ones the window does not hold). An update keeps the DOM
// node of every block whose id is unchanged (selection, embeds and state stay), replaces the rest, and keeps the
// reader's place: the first block in view stays where it was; if it was replaced, the nearest block above it that
// survived does. A fresh document is built in slices so the first screen shows at once and the rest follows.
// Work that only matters near the screen (links, code references, embeds) runs when a block comes within one viewport.

import type { Block } from "../../api";

export type BlockHooks = {
  /** Structure added right after a block is created; returns the block's root (it may be a wrapper). */
  enhance(element: HTMLElement): HTMLElement;
  /** A block came near the screen: its deferred work. Returns what undoes it. */
  near(element: HTMLElement): (() => void) | void;
};

/** Blocks built before the first paint of a fresh document. */
export const FIRST_SLICE = 120;
/** Most blocks built per turn after that (a turn also ends when it has taken [`SLICE_MS`]). */
export const NEXT_SLICE = 2000;
/** Time one turn of building may take. */
export const SLICE_MS = 8;

type Mark = { id: string; offset: number };

export type Applied = {
  /** Ids without HTML that the host does not hold (the caller asks again with nothing known). */
  missing: string[];
  /** Resolves when every block is in the DOM. */
  settled: Promise<void>;
};

export class BlockHost {
  #container: HTMLElement | null = null;
  #scroller: HTMLElement | null = null;
  #hooks: BlockHooks | null = null;
  #els = new Map<string, HTMLElement>();
  #order: string[] = [];
  #undo = new Map<string, () => void>();
  #observer: IntersectionObserver | null = null;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #sliceDone: (() => void) | null = null;
  #firstSlice = FIRST_SLICE;
  #nextSlice = NEXT_SLICE;

  /** Tests build in small slices. */
  constructor(slices: { first: number; next: number } = { first: FIRST_SLICE, next: NEXT_SLICE }) {
    this.#firstSlice = slices.first;
    this.#nextSlice = slices.next;
  }

  attach(container: HTMLElement, scroller: HTMLElement, hooks: BlockHooks): void {
    this.detach();
    this.#container = container;
    this.#scroller = scroller;
    this.#hooks = hooks;
    if (typeof IntersectionObserver !== "undefined") {
      this.#observer = new IntersectionObserver((entries) => this.#onView(entries), {
        root: scroller,
        // one viewport above and below counts as near
        rootMargin: "100% 0px 100% 0px",
      });
    }
  }

  /** Stops, undoes every block's deferred work and forgets the blocks (their DOM goes with the container). */
  detach(): void {
    this.#stopSlices();
    this.#observer?.disconnect();
    this.#observer = null;
    for (const undo of this.#undo.values()) undo();
    this.#undo.clear();
    this.#els.clear();
    this.#order = [];
    this.#container = null;
    this.#scroller = null;
    this.#hooks = null;
  }

  /** Ids of the blocks in the DOM now, in order. */
  ids(): string[] {
    return [...this.#order];
  }

  element(id: string): HTMLElement | undefined {
    return this.#els.get(id);
  }

  /** The element with this `id` attribute (a heading, a footnote) inside the document, if it is built. */
  byAnchor(id: string): HTMLElement | null {
    if (this.#container === null || id === "") return null;
    const quoted = id.replace(/["\\]/g, "\\$&");
    return this.#container.querySelector<HTMLElement>(`[id="${quoted}"]`);
  }

  get container(): HTMLElement | null {
    return this.#container;
  }

  /** The pane that scrolls the document. */
  get scroller(): HTMLElement | null {
    return this.#scroller;
  }

  /** Shows `blocks`. `fresh`: another document, everything is rebuilt (all blocks must carry HTML). */
  apply(blocks: readonly Block[], fresh: boolean): Applied {
    const container = this.#container;
    const hooks = this.#hooks;
    if (container === null || hooks === null) return { missing: [], settled: Promise.resolve() };
    this.#stopSlices();
    if (fresh) this.#clear();
    const marks = fresh ? [] : this.#marks();
    const wanted = new Set(blocks.map((b) => b.id));
    for (const id of [...this.#els.keys()]) {
      if (!wanted.has(id)) this.#drop(id);
    }
    const missing: string[] = [];
    const order: string[] = [];
    const todo: { block: Block; html: string }[] = [];
    for (const block of blocks) {
      if (this.#els.has(block.id)) order.push(block.id);
      else if (block.html === null) missing.push(block.id);
      else {
        order.push(block.id);
        todo.push({ block, html: block.html });
      }
    }
    // an update builds at once; a fresh document in slices
    const first = fresh ? this.#firstSlice : todo.length;
    this.#build(todo.slice(0, first));
    const rest = todo.slice(first);
    const built = new Set(this.#els.keys());
    this.#order = order.filter((id) => built.has(id));
    this.#place(this.#order);
    if (!fresh) this.#restore(marks);
    const settled =
      rest.length === 0 ? Promise.resolve() : new Promise<void>((done) => this.#slices(rest, done));
    return { missing, settled };
  }

  // ---- building ----------------------------------------------------------------------------------------------

  #build(items: readonly { block: Block; html: string }[]): void {
    const hooks = this.#hooks;
    if (hooks === null) return;
    const template = document.createElement("template");
    for (const { block, html } of items) {
      template.innerHTML = html;
      const first = template.content.firstElementChild;
      if (!(first instanceof HTMLElement)) continue;
      const root = hooks.enhance(first);
      root.setAttribute("data-b", block.id);
      this.#els.set(block.id, root);
      this.#observer?.observe(root);
      if (this.#observer === null) this.#laterNear(block.id, root);
    }
  }

  /** Without an observer (no viewport to watch), blocks do their deferred work as soon as they are built. */
  #laterNear(id: string, root: HTMLElement): void {
    const undo = this.#hooks?.near(root);
    if (undo) this.#undo.set(id, undo);
  }

  /** Puts the built blocks in the container in `order`, moving only what is out of place. */
  #place(order: readonly string[]): void {
    const container = this.#container;
    if (container === null) return;
    const elements: HTMLElement[] = [];
    for (const id of order) {
      const el = this.#els.get(id);
      if (el) elements.push(el);
    }
    let cursor: Element | null = container.firstElementChild;
    if (cursor === null) {
      const fragment = document.createDocumentFragment();
      for (const el of elements) fragment.append(el);
      container.append(fragment);
      return;
    }
    for (const el of elements) {
      if (el === cursor) cursor = cursor.nextElementSibling;
      else container.insertBefore(el, cursor);
    }
  }

  #slices(rest: readonly { block: Block; html: string }[], done: () => void): void {
    let at = 0;
    this.#sliceDone = done;
    const step = (): void => {
      this.#timer = null;
      // as many blocks as fit in the time one frame can spare (a few at least), so the window stays responsive
      const started = performance.now();
      const chunk: { block: Block; html: string }[] = [];
      while (at < rest.length && chunk.length < this.#nextSlice) {
        const group = rest.slice(at, at + 25);
        at += group.length;
        this.#build(group);
        chunk.push(...group);
        if (performance.now() - started > SLICE_MS) break;
      }
      const container = this.#container;
      const fragment = document.createDocumentFragment();
      for (const { block } of chunk) {
        const el = this.#els.get(block.id);
        if (!el) continue;
        fragment.append(el);
        this.#order.push(block.id);
      }
      container?.append(fragment);
      if (at < rest.length) {
        this.#timer = setTimeout(step, 0);
      } else {
        this.#sliceDone = null;
        done();
      }
    };
    this.#timer = setTimeout(step, 0);
  }

  /** Stops the slices; whoever waits for them is released (they check whether the view is still theirs). */
  #stopSlices(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
    const done = this.#sliceDone;
    this.#sliceDone = null;
    done?.();
  }

  // ---- removing ----------------------------------------------------------------------------------------------

  #drop(id: string): void {
    const el = this.#els.get(id);
    if (el) {
      this.#observer?.unobserve(el);
      el.remove();
    }
    this.#undo.get(id)?.();
    this.#undo.delete(id);
    this.#els.delete(id);
  }

  #clear(): void {
    for (const id of [...this.#els.keys()]) this.#drop(id);
    this.#order = [];
    this.#container?.replaceChildren();
  }

  // ---- the reader's place ------------------------------------------------------------------------------------

  /** The first block in view and the ones above it (nearest first), each with its distance from the pane top. */
  #marks(): Mark[] {
    const scroller = this.#scroller;
    if (scroller === null || this.#order.length === 0) return [];
    const top = scroller.getBoundingClientRect().top;
    let first = this.#order.findIndex((id) => {
      const el = this.#els.get(id);
      return el !== undefined && el.getBoundingClientRect().bottom > top + 1;
    });
    if (first < 0) first = this.#order.length - 1;
    const marks: Mark[] = [];
    for (let at = first; at >= 0 && marks.length < 12; at--) {
      const id = this.#order[at];
      const el = id === undefined ? undefined : this.#els.get(id);
      if (id !== undefined && el) marks.push({ id, offset: el.getBoundingClientRect().top - top });
    }
    return marks;
  }

  /** Puts the first surviving mark back at the distance it had; with none left the scroll position stays. */
  #restore(marks: readonly Mark[]): void {
    const scroller = this.#scroller;
    if (scroller === null) return;
    const top = scroller.getBoundingClientRect().top;
    for (const mark of marks) {
      const el = this.#els.get(mark.id);
      if (!el?.isConnected) continue;
      const delta = el.getBoundingClientRect().top - top - mark.offset;
      if (Math.abs(delta) > 0.5) scroller.scrollTop += delta;
      return;
    }
  }

  // ---- near the screen ---------------------------------------------------------------------------------------

  #onView(entries: IntersectionObserverEntry[]): void {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      const el = entry.target;
      if (!(el instanceof HTMLElement)) continue;
      this.#observer?.unobserve(el);
      const id = el.getAttribute("data-b");
      if (id === null) continue;
      this.#laterNear(id, el);
    }
  }
}
