// Correction review of one window: the piece review of `local-ai` (runs, news, suggestions in the text, accept |
// reject, marks: `local-ai/pieces/review.svelte.ts`) set up as a correction. Every prose piece of the scope goes to
// `correction.run` as `local-ai/pieces/segment.ts` cuts it (its default size); each answer -> markers back + guards
// (`guards.ts`) -> one suggestion per changed word run (`diff.ts`).

import { PieceReview, type ReviewConfig } from "../local-ai/pieces/review.svelte";
import { pieces } from "../local-ai/pieces/segment";
import { judge } from "./guards";

/** What makes the piece review a correction. */
export const REVIEW: ReviewConfig = {
  plugin: "correction",
  prepare: pieces,
  judge,
  // the model gets the pieces alone
  runArgs: () => ({}),
};

/** The review of this window. */
export const correction = new PieceReview(REVIEW);
