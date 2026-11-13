// Earlier versions asked for a file that may not be the document shown (the Explorer menu). A list is of the document
// shown, so the window goes to the file first and its list opens once the window is on it.

import { untrack } from "svelte";
import { documents } from "../../core/doc/document.svelte";
import { confirmLeave } from "../../core/editing/guard";
import { review } from "./store.svelte";

/** The file whose list opens when the window shows it, and the document shown when it was asked for. */
type Asked = { path: string; from: string | null };

let asked = $state.raw<Asked | null>(null);

/**
 * Opens the list of earlier versions of `path`: at once when it is the document shown, else after the window went to
 * it. The person keeps editing the document open: nothing opens.
 */
export async function versionsOf(path: string): Promise<void> {
  if (documents.path === path) return review.openVersions();
  // asked here, before the move asks: a "keep editing" must leave no list waiting for a later visit of the file
  if (!(await confirmLeave({ kind: "document", path }))) return;
  asked = { path, from: documents.path };
  documents.navigate(path, null);
}

/** Opens the list a file was asked for once the window shows that file, until the returned teardown runs. */
export function followVersionsOf(): () => void {
  return $effect.root(() => {
    $effect(() => {
      const wanted = asked;
      // the document shown changes with the state of the page
      void documents.state;
      const shown = documents.path;
      if (wanted === null || shown === wanted.from) return;
      untrack(() => {
        asked = null;
        // on another file than the one asked for, the person moved on: its list is not wanted any more
        if (shown === wanted.path) void review.openVersions();
      });
    });
    return () => {
      asked = null;
    };
  });
}
