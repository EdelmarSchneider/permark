// The sections of the outline that hold a change of the open review (#9): the outline marks them with a dot while
// the review of the shown version is open (blocks only; a removal leaves no block to mark).

import { documents } from "../../core/doc/document.svelte";
import { changedSections } from "../../core/doc/outline";
import { review, type ReviewStore } from "./store.svelte";

export class ReviewSections {
  readonly #store: ReviewStore;

  constructor(store: ReviewStore = review) {
    this.#store = store;
  }

  /** Outline items whose sections hold an added | edited block of the review on screen. */
  readonly changed = $derived.by((): ReadonlySet<number> => {
    const view = this.#store.reviewing;
    const doc = documents.view;
    if (
      view === null ||
      view.unit !== "blocks" ||
      doc === null ||
      doc.body.kind !== "markdown" ||
      doc.hash !== view.hash
    ) {
      return new Set();
    }
    const blocks = doc.body.blocks;
    const ids = view.changes.flatMap((change) =>
      blocks.slice(change.start, change.start + change.len).map((block) => block.id),
    );
    return changedSections(doc.body.outline, blocks, ids);
  });
}

export const reviewSections = new ReviewSections();
