// The looks a tag | an official name can wear (pure): the palette (tokens `--tag-<name>`) and the small icon set, in the
// order the pickers offer them, with the interface's name of each. Shared by the tags plugin and the official lists
// (`projects.md` §7.1: colors | icons = the tag style sets).

import type { MessageKey } from "../i18n";

/** The palette, in the order the style popover offers it (tokens `--tag-<name>`). */
export const COLORS = [
  "gray",
  "red",
  "orange",
  "yellow",
  "green",
  "teal",
  "blue",
  "purple",
] as const;
export type TagColor = (typeof COLORS)[number];

/** The icons a tag can wear (lucide names). */
export const ICONS = [
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
] as const;
export type TagIconName = (typeof ICONS)[number];

/** The interface's name of each color and icon. */
export const COLOR_LABEL: Record<TagColor, MessageKey> = {
  gray: "tags.color.gray",
  red: "tags.color.red",
  orange: "tags.color.orange",
  yellow: "tags.color.yellow",
  green: "tags.color.green",
  teal: "tags.color.teal",
  blue: "tags.color.blue",
  purple: "tags.color.purple",
};

export const ICON_LABEL: Record<TagIconName, MessageKey> = {
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

export function isColor(name: string | null | undefined): name is TagColor {
  return COLORS.some((c) => c === name);
}

export function isIcon(name: string | null | undefined): name is TagIconName {
  return ICONS.some((i) => i === name);
}
