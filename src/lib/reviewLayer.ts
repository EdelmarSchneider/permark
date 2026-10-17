// The review layer over a document the reading view already drew (docs/design/review-changes-ui.md section 3).
// It wraps the rendered blocks and inserts the old ones; it never renders the document again.

import { parseSourcepos, type LineRange } from "./sourcepos";
import type { ReviewBlock, ReviewChange, ReviewView } from "./types";

export type ReviewLabels = {
  kind: (change: ReviewChange, index: number, total: number) => string;
  group: (change: ReviewChange, index: number, total: number) => string;
  removed: (count: number, preview: string) => string;
  show: string;
  hide: string;
  keep: (index: number, total: number) => string;
  revert: (index: number, total: number) => string;
  keepHint: string;
  revertHint: string;
};

export type ReviewLayerOptions = {
  current: number;
  /** Removals left open, by the start byte of their old range, so a refresh keeps them open. */
  expanded: ReadonlySet<number>;
  labels: ReviewLabels;
  onCurrent: (index: number) => void;
  onKeep: (index: number) => void;
  onRevert: (index: number) => void;
  onToggle: (startByte: number, open: boolean) => void;
  /** Runs on each old block after it is inserted (code, math, diagrams). */
  enhance?: (root: HTMLElement) => void;
};

const PREVIEW = 60;

function linesOf(change: ReviewChange): LineRange | null {
  const blocks = change.rows.flatMap((row) => (row.new ? [row.new] : []));
  if (blocks.length === 0) return null;
  return { startLine: blocks[0].startLine, endLine: blocks[blocks.length - 1].endLine };
}

function overlaps(block: LineRange, change: LineRange): boolean {
  return block.startLine <= change.endLine && block.endLine >= change.startLine;
}

function blockLines(el: Element): LineRange | null {
  return parseSourcepos(el.getAttribute("data-sourcepos"));
}

/** The top-level blocks the reading view drew, in order. */
function renderedBlocks(body: HTMLElement): HTMLElement[] {
  return [...body.children].filter(
    (el): el is HTMLElement => el instanceof HTMLElement && blockLines(el) !== null,
  );
}

/** Puts `wrapper` in front of `anchor`, or after the change that already holds it, or at the end. */
function place(body: HTMLElement, anchor: HTMLElement | undefined, wrapper: HTMLElement): void {
  if (!anchor) {
    body.append(wrapper);
    return;
  }
  if (anchor.parentElement === body) {
    anchor.before(wrapper);
    return;
  }
  let node: HTMLElement = anchor;
  while (node.parentElement && node.parentElement !== body) node = node.parentElement;
  node.after(wrapper);
}

function htmlOf(block: ReviewBlock): HTMLElement {
  const holder = document.createElement("div");
  if (block.html.trim() !== "") holder.innerHTML = block.html;
  else {
    const pre = document.createElement("pre");
    pre.textContent = block.text;
    holder.append(pre);
  }
  const node = holder.firstElementChild;
  return node instanceof HTMLElement ? node : holder;
}

function side(block: ReviewBlock, className: string, enhance?: (root: HTMLElement) => void): HTMLElement {
  const wrap = document.createElement(className === "rv-old" ? "del" : "ins");
  wrap.className = className;
  const drawn = htmlOf(block);
  wrap.append(drawn);
  enhance?.(drawn);
  return wrap;
}

function button(
  className: string,
  label: string,
  hint: string,
  icon: string,
  onClick: () => void,
): HTMLButtonElement {
  const el = document.createElement("button");
  el.type = "button";
  el.className = className;
  el.setAttribute("aria-label", label);
  el.title = hint;
  el.textContent = icon;
  el.addEventListener("click", (event) => {
    event.stopPropagation();
    onClick();
  });
  return el;
}

function labelRow(
  change: ReviewChange,
  index: number,
  total: number,
  labels: ReviewLabels,
  onKeep: () => void,
  onRevert: () => void,
): HTMLElement {
  const row = document.createElement("div");
  row.className = "rv-label";
  const words = document.createElement("span");
  words.textContent = labels.kind(change, index, total);
  const actions = document.createElement("span");
  actions.className = "rv-label__actions";
  actions.append(
    button("rv-keep", labels.keep(index, total), labels.keepHint, "✓", onKeep),
    button("rv-revert", labels.revert(index, total), labels.revertHint, "↶", onRevert),
  );
  row.append(words, actions);
  return row;
}

function removedMarker(
  change: ReviewChange,
  open: boolean,
  labels: ReviewLabels,
  onToggle: (open: boolean) => void,
  enhance?: (root: HTMLElement) => void,
): HTMLElement {
  const blocks = change.rows.flatMap((row) => (row.old ? [row.old] : []));
  const preview = (blocks[0]?.text ?? "").replace(/\s+/g, " ").trim().slice(0, PREVIEW);
  const marker = document.createElement("div");
  marker.className = "rv-removed";
  const bar = document.createElement("button");
  bar.type = "button";
  bar.className = "rv-removed__bar";
  bar.setAttribute("aria-expanded", String(open));
  bar.textContent = `${open ? "▾" : "▸"} ${labels.removed(blocks.length, preview)}`;
  const content = document.createElement("div");
  content.className = "rv-removed__content";
  content.hidden = !open;
  for (const block of blocks) content.append(side(block, "rv-old", enhance));
  bar.addEventListener("click", (event) => {
    event.stopPropagation();
    const next = bar.getAttribute("aria-expanded") !== "true";
    bar.setAttribute("aria-expanded", String(next));
    content.hidden = !next;
    bar.textContent = `${next ? "▾" : "▸"} ${labels.removed(blocks.length, preview)}`;
    onToggle(next);
  });
  marker.append(bar, content);
  return marker;
}

/**
 * Decorates `body` with one wrapper per change. Call it on a document that was just rendered: it moves the
 * rendered blocks into the wrappers.
 */
export function decorateReview(body: HTMLElement, view: ReviewView, options: ReviewLayerOptions): void {
  clearReview(body);
  const blocks = renderedBlocks(body);
  const used = new Set<HTMLElement>();
  const total = view.changes.length;

  view.changes.forEach((change, index) => {
    const wrapper = document.createElement("div");
    wrapper.className = "rv-change";
    wrapper.dataset.kind = change.kind;
    wrapper.dataset.index = String(index);
    wrapper.tabIndex = -1;
    wrapper.setAttribute("role", "group");
    wrapper.setAttribute("aria-label", options.labels.group(change, index, total));
    if (index === options.current) wrapper.classList.add("is-current");
    wrapper.addEventListener("click", () => options.onCurrent(index));
    wrapper.append(
      labelRow(
        change,
        index,
        total,
        options.labels,
        () => options.onKeep(index),
        () => options.onRevert(index),
      ),
    );

    const span = linesOf(change);
    const owned =
      span === null
        ? []
        : blocks.filter((block) => {
            if (used.has(block)) return false;
            const lines = blockLines(block);
            return lines !== null && overlaps(lines, span);
          });
    owned.forEach((block) => used.add(block));

    if (change.kind === "removed") {
      wrapper.append(
        removedMarker(
          change,
          options.expanded.has(change.old.startByte),
          options.labels,
          (open) => options.onToggle(change.old.startByte, open),
          options.enhance,
        ),
      );
      const anchor =
        change.newLine === null
          ? undefined
          : blocks.find((block) => {
              const lines = blockLines(block);
              return lines !== null && lines.startLine >= change.newLine!;
            });
      place(body, anchor, wrapper);
      return;
    }

    place(body, owned[0], wrapper);

    if (change.kind === "added") {
      for (const block of owned) {
        const ins = document.createElement("ins");
        ins.className = "rv-new";
        block.before(ins);
        ins.append(block);
        wrapper.append(ins);
      }
      return;
    }

    let nextOwned = 0;
    for (const row of change.rows) {
      if (row.old) wrapper.append(side(row.old, "rv-old", options.enhance));
      if (row.new) {
        const block = owned[nextOwned++];
        const ins = document.createElement("ins");
        ins.className = "rv-new";
        if (block) {
          ins.append(block);
        } else {
          ins.append(htmlOf(row.new));
        }
        wrapper.append(ins);
      }
    }
  });
}

/** Takes the review layer off, putting the rendered blocks back where they were. */
export function clearReview(body: HTMLElement): void {
  for (const wrapper of [...body.querySelectorAll<HTMLElement>(".rv-change")]) {
    for (const ins of [...wrapper.querySelectorAll("ins.rv-new")]) {
      const block = ins.firstElementChild;
      if (block) wrapper.before(block);
    }
    wrapper.remove();
  }
}
