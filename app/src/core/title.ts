// Window title: tells windows apart in the task bar | Alt+Tab. `<doc> — <project> — PerMark`.

import type { WindowView } from "../api";
import { t } from "./i18n";

function lastSegment(path: string): string {
  const parts = path.split("/").filter((p) => p !== "");
  return parts[parts.length - 1] ?? path;
}

export function windowTitle(view: WindowView): string {
  switch (view.state) {
    case "welcome":
    case "closing":
      return t("app.name");
    case "opening":
      return view.showing ? workspaceTitle(view.showing, null) : t("app.name");
    case "showing":
      return workspaceTitle(view.workspace, view.document);
    default:
      return view satisfies never;
  }
}

function workspaceTitle(
  workspace: Extract<WindowView, { state: "showing" }>["workspace"],
  document: string | null,
): string {
  switch (workspace.kind) {
    case "project":
      return document
        ? t("window.title.project", { document: lastSegment(document), project: workspace.name })
        : t("window.title.projectOnly", { project: workspace.name });
    case "singleFile":
      return t("window.title.file", { document: workspace.name });
    default:
      return workspace satisfies never;
  }
}
