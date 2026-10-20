// Where the Notes are, for the commands and the top-bar button: is this view the Notes tab. Pure.

import type { WindowView } from "../../api";

/** This view shows the Notes (the Notes tab). */
export function inNotes(view: WindowView): boolean {
  switch (view.state) {
    case "showing":
      return view.workspace.kind === "notes";
    case "opening":
    case "welcome":
    case "closing":
      return false;
    default:
      return view satisfies never;
  }
}
