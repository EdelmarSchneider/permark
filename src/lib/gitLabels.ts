import { t } from "./i18n";
import type { GitState } from "./types";

/** Tooltip / accessible name of a git state badge. */
export function gitStateLabel(state: GitState): string {
  switch (state) {
    case "modified":
      return t("git.state.modified");
    case "added":
      return t("git.state.added");
    case "untracked":
      return t("git.state.untracked");
    case "deleted":
      return t("git.state.deleted");
    case "renamed":
      return t("git.state.renamed");
    case "conflicted":
      return t("git.state.conflicted");
  }
}
