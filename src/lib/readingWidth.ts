// The reading width: how wide the text column of a document is (docs/design/v0.2.md section 15).

export const READING_WIDTHS = ["comfortable", "wide", "full"] as const;
export type ReadingWidth = (typeof READING_WIDTHS)[number];

export const DEFAULT_WIDTH: ReadingWidth = "comfortable";

export function isReadingWidth(value: unknown): value is ReadingWidth {
  return typeof value === "string" && (READING_WIDTHS as readonly string[]).includes(value);
}

/**
 * The width to use for a stored session. A saved `width` wins; a session from before v0.2 only has the
 * on/off `wide` (true was the 1100 px column); anything this build does not understand is the default.
 */
export function normalizeWidth(width: unknown, legacyWide?: unknown): ReadingWidth {
  if (isReadingWidth(width)) return width;
  return legacyWide === true ? "wide" : DEFAULT_WIDTH;
}

/** The next width in the cycle Comfortable, Wide, Full, and back to Comfortable. */
export function nextWidth(width: ReadingWidth): ReadingWidth {
  return READING_WIDTHS[(READING_WIDTHS.indexOf(width) + 1) % READING_WIDTHS.length];
}
