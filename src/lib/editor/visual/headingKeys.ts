// Paragraph and heading shortcuts for Visual mode. Ctrl+Alt is the AltGr key on Windows, so a binding
// written as Ctrl+Alt+1 never sees the character "1" on an ABNT-2 keyboard (that chord types ¹). These
// chords match the physical key (`Digit1`, `Numpad1`), not the character the layout produces.

/** The block Ctrl+Shift+0 through Ctrl+Shift+6 sets. */
export type HeadingShortcut = "paragraph" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6";

/** The fields of a keydown that choose a block. `code` is the physical key; `key` is what was typed. */
export type HeadingKeyEvent = {
  key: string;
  code?: string;
  ctrlKey: boolean;
  metaKey?: boolean;
  altKey: boolean;
  shiftKey: boolean;
};

const PHYSICAL_DIGIT = /^(?:Digit|Numpad)([0-6])$/;

/**
 * The block for Ctrl+Shift+0–6 (Command+Shift on macOS), or null when this is not that chord.
 * Alt must be up: Ctrl+Alt+digit is AltGr, and it is not a heading shortcut.
 * A null result leaves the character to be typed.
 */
export function matchHeadingShortcut(event: HeadingKeyEvent): HeadingShortcut | null {
  const mod = event.ctrlKey || event.metaKey === true;
  if (!mod || !event.shiftKey || event.altKey) return null;
  const digit = PHYSICAL_DIGIT.exec(event.code ?? "");
  if (!digit) return null;
  const n = Number(digit[1]);
  if (n === 0) return "paragraph";
  return `h${n}` as HeadingShortcut;
}

/**
 * When `event` is a heading chord, runs `apply` and reports that the key was taken.
 * A taken key is not left for insertion: the editor calls preventDefault when this returns true.
 */
export function takeHeadingShortcut(
  event: HeadingKeyEvent,
  apply: (block: HeadingShortcut) => void,
): boolean {
  const block = matchHeadingShortcut(event);
  if (block === null) return false;
  apply(block);
  return true;
}
