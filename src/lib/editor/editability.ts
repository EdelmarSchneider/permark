// Which documents can be edited, and why not (docs/design/v0.2.md section 3: "[Edit] is disabled with a tooltip
// saying why"). The backend refuses the same files; this only lets the UI say so before asking.

import type { MessageKey } from "../i18n";
import type { DocumentView } from "../types";

export type EditBlock = "binary" | "tooLarge" | "nonMarkdown" | "encoding";

/** Why a document cannot be edited, or null when it can. */
export function editBlock(
  view: Pick<DocumentView, "kind" | "encodingNote"> | null,
): EditBlock | null {
  if (!view) return null;
  if (view.kind === "binary") return "binary";
  if (view.kind === "tooLarge") return "tooLarge";
  if (view.kind !== "markdown") return "nonMarkdown";
  // UTF-16, or bytes that are not UTF-8: saving would write UTF-8 over them
  if (view.encodingNote) return "encoding";
  return null;
}

export const EDIT_BLOCK_TEXT: Record<EditBlock, MessageKey> = {
  binary: "edit.disabledBinary",
  tooLarge: "edit.disabledTooLarge",
  nonMarkdown: "edit.disabledNonMarkdown",
  encoding: "edit.disabledEncoding",
};

/** The backend's error kinds that mean the same as an `EditBlock` (the UI could not tell beforehand). */
export function editBlockFromKind(kind: string | null): EditBlock | null {
  switch (kind) {
    case "binary":
      return "binary";
    case "tooLarge":
      return "tooLarge";
    case "notMarkdown":
      return "nonMarkdown";
    case "encoding":
      return "encoding";
    default:
      return null;
  }
}
