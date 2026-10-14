// How a document opens for editing (docs/design/v0.2.md section 3): the visual editor or the Markdown source. The choice is the
// session's `editMode`; the segmented switch in the header changes the mode of the document being edited.

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
