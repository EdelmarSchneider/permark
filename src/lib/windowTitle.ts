// The title of the window, as a pure function of what the window shows. It is what the taskbar, Alt+Tab and the taskbar thumbnails
// list, so windows can be told apart: the document, then the project (or "Notes"), then the program, with a `*` before it all
// while the document has unsaved edits. The whole format of each case is one string of the language's table, so a translation
// can put the pieces in another order.

import { t } from "./i18n";

/** What the title depends on. `document` is the name the window shows for the document (a file name, a note's title), if one is open. */
export type TitleState =
  /** The welcome screen, and the moment before the window knows what it is. */
  | { screen: "welcome" }
  | {
      screen: "project";
      /** One file opened on its own: the folder it is in is not the person's project, so it is not named. */
      simple: boolean;
      /** The folder of the project. */
      project: string | null;
      document: string | null;
      unsaved: boolean;
    }
  | { screen: "notes"; document: string | null; unsaved: boolean };

/** The title for a window in `state`, in the language of the interface. */
export function windowTitle(state: TitleState): string {
  const title = baseTitle(state);
  const unsaved = state.screen !== "welcome" && state.unsaved && state.document !== null;
  return unsaved ? t("window.title.unsaved", { title }) : title;
}

function baseTitle(state: TitleState): string {
  if (state.screen === "welcome") return t("app.name");
  const document = state.document;
  if (state.screen === "notes") {
    return document === null ? t("window.title.notesOnly") : t("window.title.notes", { document });
  }
  // a file opened on its own is not in a project of the person's, so no folder is named
  const project = state.simple ? "" : (state.project ?? "");
  if (project === "") {
    return document === null ? t("app.name") : t("window.title.file", { document });
  }
  return document === null
    ? t("window.title.projectOnly", { project })
    : t("window.title.project", { document, project });
}
