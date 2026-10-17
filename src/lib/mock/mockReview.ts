// Review of changes for the mock backend: the mock project has no history to compare with, so every document is as approved.

import type { ReviewEntry, ReviewView } from "../types";

/** The documents that differ from their approved version: none. */
export const reviewList = (): ReviewEntry[] => [];

/** A document with nothing to review. */
export const reviewGet = (path: string): ReviewView => ({
  path,
  status: "unchanged",
  hash: null,
  reviewedHash: null,
  restorable: false,
  changes: [],
});
