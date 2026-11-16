// The page setup offered before the person chose one: the paper sold in their region.

import type { PageSetup } from "../../api/generated/PageSetup";
import type { PageSize } from "../../api/generated/PageSize";
import type { SetupView } from "../../api/generated/SetupView";

/** Regions where US Letter is the common paper (the rest of the world uses A4). */
const LETTER_REGIONS = new Set([
  "US",
  "CA",
  "MX",
  "PH",
  "CL",
  "CO",
  "VE",
  "CR",
  "GT",
  "PA",
  "DO",
  "PR",
  "SV",
  "NI",
]);

/**
 * The paper of the region a language tag names (`en-US` -> Letter). Only a region written in the tag counts: a bare
 * language (`en`, `es`) is spoken on both papers' sides, so it keeps A4.
 */
export function regionPaper(language: string): PageSize {
  let region: string | undefined;
  try {
    region = new Intl.Locale(language).region;
  } catch {
    return "a4";
  }
  return region !== undefined && LETTER_REGIONS.has(region) ? "letter" : "a4";
}

/** The setup to offer: the stored one, or the default with the region's paper when nothing was chosen yet. */
export function offeredSetup(view: SetupView, language: string): PageSetup {
  return view.stored ? view.setup : { ...view.setup, size: regionPaper(language) };
}
