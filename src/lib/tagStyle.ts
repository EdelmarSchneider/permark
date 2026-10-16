// How a tag looks: an optional color from a fixed palette and an optional icon from a small set. The palette's
// colors are design tokens (`--tag-<color>` in styles/tokens.css) and the icons are names of the app's own icon set.
// The backend keeps the same two lists (`src-tauri/src/meta/style.rs`); a test compares them.

import type { IconName } from "./components/Icon.svelte";
import type { MessageKey } from "./i18n";

export const TAG_COLORS = [
  "gray",
  "red",
  "orange",
  "yellow",
  "green",
  "teal",
  "blue",
  "purple",
] as const;
export type TagColor = (typeof TAG_COLORS)[number];

export const TAG_ICONS = [
  "star",
  "bookmark",
  "flag",
  "lightbulb",
  "check",
  "alert",
  "book-open",
  "code",
  "folder",
  "user",
  "calendar",
  "link",
] as const satisfies readonly IconName[];
export type TagIcon = (typeof TAG_ICONS)[number];

/** The look of one tag: either part can be missing. A tag with neither looks as every tag did before. */
export type TagStyle = { color: TagColor | null; icon: TagIcon | null };

/** The look of every tag that has one, by tag name. */
export type TagStyles = ReadonlyMap<string, TagStyle>;

export const NO_STYLES: TagStyles = new Map();

/** The words the pickers and the tooltips use for each color and icon. */
export const COLOR_LABELS: Record<TagColor, MessageKey> = {
  gray: "tags.color.gray",
  red: "tags.color.red",
  orange: "tags.color.orange",
  yellow: "tags.color.yellow",
  green: "tags.color.green",
  teal: "tags.color.teal",
  blue: "tags.color.blue",
  purple: "tags.color.purple",
};
export const ICON_LABELS: Record<TagIcon, MessageKey> = {
  star: "tags.icon.star",
  bookmark: "tags.icon.bookmark",
  flag: "tags.icon.flag",
  lightbulb: "tags.icon.lightbulb",
  check: "tags.icon.check",
  alert: "tags.icon.alert",
  "book-open": "tags.icon.bookOpen",
  code: "tags.icon.code",
  folder: "tags.icon.folder",
  user: "tags.icon.user",
  calendar: "tags.icon.calendar",
  link: "tags.icon.link",
};

export function isTagColor(value: unknown): value is TagColor {
  return TAG_COLORS.some((c) => c === value);
}

export function isTagIcon(value: unknown): value is TagIcon {
  return TAG_ICONS.some((i) => i === value);
}

/** A look from untrusted data (the backend, a mock): a name outside the palette or the set is no color or icon. */
export function readStyle(raw: unknown): TagStyle | null {
  if (typeof raw !== "object" || raw === null) return null;
  const { color, icon } = raw as Record<string, unknown>;
  const style: TagStyle = {
    color: isTagColor(color) ? color : null,
    icon: isTagIcon(icon) ? icon : null,
  };
  return hasLook(style) ? style : null;
}

/** The looks the backend answered, by tag name; anything it says that this build cannot draw is left out. */
export function readStyles(raw: unknown): TagStyles {
  const styles = new Map<string, TagStyle>();
  if (typeof raw !== "object" || raw === null) return styles;
  for (const [tag, value] of Object.entries(raw)) {
    const style = readStyle(value);
    if (style) styles.set(tag, style);
  }
  return styles;
}

/** Whether a look changes anything. */
export function hasLook(style: TagStyle | null | undefined): style is TagStyle {
  return style != null && (style.color !== null || style.icon !== null);
}

/**
 * Where an arrow key (or Home or End) moves in a row or grid of `count` choices laid out `columns` wide: the index of the
 * choice to select next, or `null` for any other key. Left and right wrap around, up and down move a row (and step like
 * left and right when everything is in one row), and the ends stay put.
 */
export function gridMove(
  key: string,
  index: number,
  count: number,
  columns: number,
): number | null {
  if (count <= 0) return null;
  const last = count - 1;
  const oneRow = columns >= count;
  switch (key) {
    case "ArrowRight":
      return index >= last ? 0 : index + 1;
    case "ArrowLeft":
      return index <= 0 ? last : index - 1;
    case "ArrowDown":
      return oneRow
        ? gridMove("ArrowRight", index, count, columns)
        : Math.min(last, index + columns);
    case "ArrowUp":
      return oneRow ? gridMove("ArrowLeft", index, count, columns) : Math.max(0, index - columns);
    case "Home":
      return 0;
    case "End":
      return last;
    default:
      return null;
  }
}

/** The looks with one tag's look replaced (`null`, or a look with neither part, removes it). */
export function withStyle(styles: TagStyles, tag: string, style: TagStyle | null): TagStyles {
  const next = new Map(styles);
  if (hasLook(style)) next.set(tag, style);
  else next.delete(tag);
  return next;
}
