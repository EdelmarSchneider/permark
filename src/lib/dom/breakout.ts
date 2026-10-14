// Helpers for blocks that may outgrow the text measure (docs/design/v0.2.md section 15).

/** The drawn width of an SVG from its `viewBox` ("0 0 812 300" -> 812); null when it has no usable one. */
export function viewBoxWidth(viewBox: string | null | undefined): number | null {
  if (!viewBox) return null;
  const parts = viewBox
    .trim()
    .split(/[\s,]+/)
    .map(Number);
  if (parts.length !== 4 || parts.some((n) => !Number.isFinite(n))) return null;
  const width = parts[2];
  return width > 0 ? Math.round(width) : null;
}
