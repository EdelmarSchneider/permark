// What the window shows, read from the stores in the form `windowTitle()` takes. Read inside an effect or a `$derived`, it makes
// the title follow the document, the project, the unsaved mark and the language.

import { t } from "../i18n";
import { isLoosePath, looseName } from "../looseFile";
import { basename } from "../paths";
import type { TitleState } from "../windowTitle";
import { app } from "./app.svelte";
import { docs } from "./docs.svelte";
import { edit } from "./edit.svelte";
import { notes } from "./notes.svelte";
import { vault } from "./vault.svelte";

/** The name the Notes window shows for the note on screen (as its header does), or null when none is. */
function noteName(): string | null {
  const path = notes.current;
  if (path === null) return null;
  const title = notes.currentEntry?.title ?? "";
  if (title !== "") return title;
  return isLoosePath(path) ? looseName(path) : t("notes.untitled");
}

export function currentTitleState(): TitleState {
  if (app.screen === "project") {
    const path = docs.path;
    return {
      screen: "project",
      simple: vault.isSimple,
      project: vault.info?.name ?? null,
      document: path === null ? null : basename(path),
      unsaved: edit.dirty,
    };
  }
  if (app.screen === "notes") {
    // a note is saved as it is typed, so only a loose file (written when asked) has edits that are not saved
    const entry = notes.currentEntry;
    return {
      screen: "notes",
      document: noteName(),
      unsaved: notes.isFile && (entry?.unsaved === true || (edit.loose && edit.dirty)),
    };
  }
  return { screen: "welcome" };
}
