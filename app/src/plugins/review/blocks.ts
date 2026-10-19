// The review as blocks of the page: the old blocks that went (or were replaced) come back as ghosts, each change gets
// a small bar with Keep | Revert. Pure: the blocks of the document + the review in, the blocks to show out.

import type { Block, ReviewChange, ReviewChangeKind, ReviewView } from "../../api";
import { t, type MessageKey } from "../../core/i18n";

/** A small stable hash (FNV-1a) of the html: the id of a synthetic block changes when what it shows changes. */
export function hashOf(text: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0).toString(16);
}

const KIND_KEYS: Readonly<Record<ReviewChangeKind, MessageKey>> = {
  edited: "review.kind.modified",
  added: "review.kind.added",
  removed: "review.kind.removed",
};

export function kindWord(kind: ReviewChangeKind): string {
  return t(`review.kindWord.${kind === "edited" ? "modified" : kind}`);
}

function escape(text: string): string {
  return text.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);
}

/** The html of a ghost: an old block, without the id it had (it is not a block of the page). */
function ghostHtml(change: ReviewChange, block: Block): string {
  const inner = (block.html ?? "").replace(/ data-b="[^"]*"/, "");
  return `<div class="rv-ghost" data-rv-change="${change.index}" aria-label="${escape(t("review.removedLabel"))}">${inner}</div>`;
}

/** The bar of one change. */
export function barHtml(
  change: ReviewChange,
  total: number,
  canKeep: boolean,
  current: boolean,
): string {
  const label = t(KIND_KEYS[change.kind], { i: change.index + 1, n: total });
  const keep = canKeep
    ? `<button type="button" data-rv-act="keep" data-rv-index="${change.index}" title="${escape(t("review.keepHint"))}">${escape(t("review.keepLabel"))}</button>`
    : "";
  const revert = `<button type="button" data-rv-act="revert" data-rv-index="${change.index}" title="${escape(t("review.revertHint"))}">${escape(t("review.revertLabel"))}</button>`;
  return `<div class="rv-bar${current ? " is-current" : ""}" data-rv-change="${change.index}" data-rv-bar role="group" aria-label="${escape(t("review.change", { i: change.index + 1, n: total, kind: kindWord(change.kind) }))}"><span class="rv-kind">${escape(label)}</span><span class="rv-actions">${keep}${revert}</span></div>`;
}

function synthetic(prefix: string, html: string, kind: string): Block {
  return { id: `rv:${prefix}:${hashOf(html)}`, kind, lineStart: 0, lineEnd: 0, html };
}

/** The blocks of the page in review: ghosts and bars where the changes are. */
export function reviewBlocks(blocks: readonly Block[], view: ReviewView, current: number): Block[] {
  const out: Block[] = [];
  const at = new Map<number, ReviewChange[]>();
  for (const change of view.changes) {
    const list = at.get(change.start) ?? [];
    list.push(change);
    at.set(change.start, list);
  }
  const place = (index: number): void => {
    for (const change of at.get(index) ?? []) {
      change.ghosts.forEach((ghost, n) => {
        out.push(synthetic(`ghost${change.index}.${n}`, ghostHtml(change, ghost), "div"));
      });
      out.push(
        synthetic(
          `bar${change.index}`,
          barHtml(change, view.count, view.canKeep, change.index === current),
          "div",
        ),
      );
    }
  };
  blocks.forEach((block, index) => {
    place(index);
    out.push(block);
  });
  place(blocks.length);
  return out;
}
