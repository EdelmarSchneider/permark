// How a move of a view is named in its options menu and said once it is done (`{name}`: the view). One table for
// the menu and for a drop: the same move says the same words whichever way it was made.

import type { MessageKey } from "../i18n";
import type { Axis, Region } from "./arrangement";

/** A move: how the menu names it, how the status line says it once done. */
export type Words = { readonly label: MessageKey; readonly said: MessageKey };

/**
 * The moves inside a region, worded by the direction its groups follow each other: one group back | forth, out of a
 * tab group into a group of its own before | after it. `folded`: why none can act while the window is too small to
 * draw the groups apart.
 */
export const ALONG: Readonly<
  Record<
    Axis,
    {
      readonly back: Words;
      readonly forth: Words;
      readonly before: Words;
      readonly after: Words;
      readonly folded: MessageKey;
    }
  >
> = {
  vertical: {
    back: { label: "panel.moveUp", said: "panel.moved.up" },
    forth: { label: "panel.moveDown", said: "panel.moved.down" },
    before: { label: "panel.showAbove", said: "panel.moved.above" },
    after: { label: "panel.showBelow", said: "panel.moved.below" },
    folded: "panel.tooShort",
  },
  horizontal: {
    back: { label: "panel.moveLeft", said: "panel.moved.left" },
    forth: { label: "panel.moveRight", said: "panel.moved.right" },
    before: { label: "panel.showOnLeft", said: "panel.moved.onLeft" },
    after: { label: "panel.showOnRight", said: "panel.moved.onRight" },
    folded: "panel.tooNarrow",
  },
};

/** Into the group beside it, as a tab. */
export const AS_TAB: Words = { label: "panel.showAsTab", said: "panel.moved.tab" };

/** The move into another region. */
export const ACROSS: Readonly<Record<Region, Words>> = {
  left: { label: "panel.moveToLeft", said: "panel.moved.toLeft" },
  right: { label: "panel.moveToRight", said: "panel.moved.toRight" },
  bottom: { label: "panel.moveToBottom", said: "panel.moved.toBottom" },
};
