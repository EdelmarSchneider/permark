// How a document opens for editing (docs/design/v0.2.md section 3): Edit or the Markdown source. The choice is the
// session's `editMode`; the segmented switch in the header (View | Edit | Source) moves between the rendered page and the two editors.
// The stored names stay `read`, `visual` and `source`.

export const EDIT_MODES = ["visual", "source"] as const;
export type EditMode = (typeof EDIT_MODES)[number];

export const DEFAULT_EDIT_MODE: EditMode = "visual";

export function isEditMode(value: unknown): value is EditMode {
  return typeof value === "string" && (EDIT_MODES as readonly string[]).includes(value);
}

/** The mode to use for a stored session; a name this build does not know (a newer UI wrote it) is the default. */
export function normalizeEditMode(value: unknown): EditMode {
  return isEditMode(value) ? value : DEFAULT_EDIT_MODE;
}

/** The three modes of the header switch, in the order it shows them: reading, or one of the editors. */
export const DOC_MODES = ["read", ...EDIT_MODES] as const;
export type DocMode = (typeof DOC_MODES)[number];

/** The editor that is not `mode` (Ctrl+Alt+E from reading enters the one that was not used last). */
export function otherEditMode(mode: EditMode): EditMode {
  return mode === "visual" ? "source" : "visual";
}

/**
 * The shortcut that goes to `target` from `current`, for the tooltip of its segment; null when there is none (it is the mode
 * already shown). From reading, Ctrl+E (and E) enter the editor used last and Ctrl+Alt+E the other one; between the editors,
 * Ctrl+Alt+E; back to reading, Ctrl+E.
 */
export function shortcutToward(target: DocMode, current: DocMode, last: EditMode): string | null {
  if (target === current) return null;
  if (target === "read") return "Ctrl+E";
  if (current === "read") return target === last ? "Ctrl+E" : "Ctrl+Alt+E";
  return "Ctrl+Alt+E";
}

/** The segment an arrow key (or Home and End) moves the keyboard focus to; the focus wraps around. Null for any other key. */
export function focusTarget(from: DocMode, key: string): DocMode | null {
  const at = DOC_MODES.indexOf(from);
  const count = DOC_MODES.length;
  switch (key) {
    case "ArrowRight":
    case "ArrowDown":
      return DOC_MODES[(at + 1) % count];
    case "ArrowLeft":
    case "ArrowUp":
      return DOC_MODES[(at + count - 1) % count];
    case "Home":
      return DOC_MODES[0];
    case "End":
      return DOC_MODES[count - 1];
    default:
      return null;
  }
}
