// The one-time offer to show code files in the tree. The default filter hides them (the tree is for readers first);
// a project that clearly holds code is asked once, quietly. Pure: the store keeps the answer.

import type { Category, TreeFilter } from "../../api";
import { categoriesOf } from "./filter";

/** The project session key that remembers the answer (either answer ends the offer for that project). */
export const CODE_OFFER_KEY = "explorer.code-offer";

/** At least this many code files are hidden... */
export const MIN_CODE_FILES = 5;
/** ...and they are at least one file in this many of the project's files. */
export const MIN_CODE_SHARE = 5;

export type Answer = "shown" | "declined";

export function isAnswer(value: unknown): value is Answer {
  return value === "shown" || value === "declined";
}

/** What the offer needs of the list. */
export type OfferInput = {
  /** Code files the filter hides. */
  codeHidden: number;
  /** Files that pass the hidden | ignored switches (shown or not). */
  totalFiles: number;
  filter: TreeFilter;
  /** What the person answered before (`null`: never asked). */
  answered: Answer | null;
};

/** Whether the project is asked about its code now. */
export function shouldOfferCode(input: OfferInput): boolean {
  const { codeHidden, totalFiles, filter, answered } = input;
  if (answered !== null) return false;
  // a filter the person made (patterns, their own categories) is theirs
  if (filter.show !== "documents" && filter.show !== "docs-and-assets") return false;
  if (filter.patterns.trim() !== "") return false;
  return codeHidden >= MIN_CODE_FILES && codeHidden * MIN_CODE_SHARE >= totalFiles;
}

/** The filter with code shown: the preset's categories and `code`, everything else as it was. */
export function withCode(filter: TreeFilter): TreeFilter {
  const base: Category[] =
    filter.show === "custom" ? [...filter.categories] : categoriesOf(filter.show);
  return {
    ...filter,
    show: "custom",
    categories: base.includes("code") ? base : [...base, "code"],
  };
}
