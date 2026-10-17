// What the letters of the git badges say, in plain words. PerMark only reads Git: these texts tell a person who never
// opens a terminal what a letter means, what (if anything) they have to do, and that the program never commits.

import { t } from "./i18n";
import { badgeFor, type Badge } from "./tree";
import type { GitSnapshot, GitState } from "./types";

/** What the letters are compared with: the commit stored at the last visit (the default), or the last commit. */
export type GitBaseline = GitSnapshot["baseline"]["kind"];

/** The states in the order the legend lists them. */
export const LEGEND_STATES: readonly GitState[] = [
  "modified",
  "added",
  "untracked",
  "deleted",
  "renamed",
  "conflicted",
];

/** One sentence that says what a state means ("Modified: this file changed since your last visit."). */
export function gitExplanation(state: GitState, baseline: GitBaseline = "lastVisit"): string {
  const since = t(baseline === "head" ? "git.since.commit" : "git.since.visit");
  switch (state) {
    case "modified":
      return t("git.explain.modified", { since });
    case "added":
      return t("git.explain.added");
    case "untracked":
      return t("git.explain.untracked");
    case "deleted":
      return t("git.explain.deleted", { since });
    case "renamed":
      return t("git.explain.renamed", { since });
    case "conflicted":
      return t("git.explain.conflicted");
  }
}

/** The tooltip of a badge: its letter, what it means, and that nothing has to be done (a conflict says what to do instead). */
export function gitBadgeTitle(badge: Badge, baseline: GitBaseline = "lastVisit"): string {
  const text = gitExplanation(badge.state, baseline);
  if (badge.state === "conflicted") return `${badge.letter} · ${text}`;
  return t("git.badgeTitle", { letter: badge.letter, text, tail: t("git.tail") });
}

export type LegendRow = { badge: Badge; text: string };

/** The rows of the legend: the badge as the tree draws it, and its sentence. */
export function legendRows(baseline: GitBaseline = "lastVisit"): LegendRow[] {
  const rows: LegendRow[] = [];
  for (const state of LEGEND_STATES) {
    const badge = badgeFor(state);
    if (badge) rows.push({ badge, text: gitExplanation(state, baseline) });
  }
  return rows;
}

/** The line under the rows: what the letters are compared with, and when they clear. */
export function legendBaselineNote(baseline: GitBaseline = "lastVisit"): string {
  return t(baseline === "head" ? "git.legend.commit" : "git.legend.visit");
}
