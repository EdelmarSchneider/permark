// Window title: tells windows apart in the task bar | Alt+Tab. `<doc> — <project> — PerMark`; `*<doc>` while the
// document has unsaved edits.

import type { WindowView } from "../api";
import { t } from "./i18n";

function lastSegment(path: string): string {
  const parts = path.split("/").filter((p) => p !== "");
  return parts[parts.length - 1] ?? path;
}

export function windowTitle(view: WindowView, unsaved = false): string {
  switch (view.state) {
    case "welcome":
    case "closing":
      return t("app.name");
    case "opening":
      return view.showing ? workspaceTitle(view.showing, null, false) : t("app.name");
    case "showing":
      return workspaceTitle(view.workspace, view.document, unsaved);
    default:
      return view satisfies never;
  }
}

function workspaceTitle(
  workspace: Extract<WindowView, { state: "showing" }>["workspace"],
  document: string | null,
  unsaved: boolean,
): string {
  const star = unsaved ? "*" : "";
  switch (workspace.kind) {
    case "project":
      return document
        ? t("window.title.project", {
            document: star + lastSegment(document),
            project: workspace.name,
          })
        : t("window.title.projectOnly", { project: workspace.name });
    case "singleFile":
      // the file shown (a link may have led to another file of its folder)
      return t("window.title.file", {
        document: star + (document ? lastSegment(document) : workspace.name),
      });
    case "notes":
      return star + t("window.title.notes");
    default:
      return workspace satisfies never;
  }
}
