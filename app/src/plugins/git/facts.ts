// What the git plugin says about an included project's repository (`projects.md` §12.3, §12.4, §12.2, §12.7), from
// its `RepoSummary`: the node's muted text, the Settings row status, the expanded row's line, the card's lines, the
// owner repository of a path. Pure (words through `t`).

import type { RepoSummary } from "../../api";
import type { RepoState } from "../../api/generated/RepoState";
import { t, tn, type MessageKey } from "../../core/i18n";
import { keyOf } from "../../core/projects/model";
import type { ProjectFact } from "../../core/plugins/registry.svelte";

/** The backend counts changed files up to this many (more: "5000+"). */
export const CHANGED_CAP = 5000;

export const STATE_KEYS: Readonly<Record<RepoState, MessageKey | null>> = {
  clean: null,
  merging: "git.state.merging",
  rebasing: "git.state.rebasing",
  cherryPicking: "git.state.cherryPicking",
  reverting: "git.state.reverting",
  bisecting: "git.state.bisecting",
  busy: "git.state.busy",
};

/** What a state chip means, in plain words (tooltip + what a screen reader hears; UX round 3). */
export const STATE_HINTS: Readonly<Record<RepoState, MessageKey | null>> = {
  clean: null,
  merging: "git.stateHint.merging",
  rebasing: "git.stateHint.rebasing",
  cherryPicking: "git.stateHint.cherryPicking",
  reverting: "git.stateHint.reverting",
  bisecting: "git.stateHint.bisecting",
  busy: "git.stateHint.busy",
};

/** The plain-words meaning of a repository's state chip; `null`: no chip (clean). */
export function stateHint(state: RepoState): string | null {
  const key = STATE_HINTS[state];
  return key === null ? null : t(key);
}

/** Seven characters of a commit id. */
export function shortOid(oid: string | null): string {
  return oid?.slice(0, 7) ?? "";
}

/** Git cannot be asked at all (no git): no branch anywhere, one sentence. */
export function gitMissing(s: RepoSummary): boolean {
  return s.error?.kind === "unavailable";
}

/** Where HEAD is, in words: the branch, "not on a branch (at a1b2c3d)", or nothing (no commit and no name; unknown). */
export function headText(s: RepoSummary): string | null {
  if (gitMissing(s)) return null;
  if (s.detached) return t("projects.node.detached", { oid: shortOid(s.oid) });
  return s.branch;
}

/** The node's muted text: what the repository is doing (a chip, its meaning in the tooltip + spoken) instead of the
 *  branch, else the branch (a branch mark before it; also the branch a repository with no commit yet is on) | detached
 *  commit. Git can't read it: a warning mark (+ the branch when known), never the look of a clean project. */
export function nodeFact(s: RepoSummary): ProjectFact | null {
  if (gitMissing(s)) return null;
  if (s.error?.kind === "failed") {
    const problem = t("projects.card.gitError");
    return { text: headText(s) ?? "", warn: true, spoken: problem, title: problem };
  }
  const state = STATE_KEYS[s.state];
  if (state !== null) {
    const text = t(state);
    const head = headText(s);
    const hint = stateHint(s.state) ?? "";
    const spokenHead =
      head === null || s.detached ? null : t("projects.node.srBranch", { branch: head });
    return {
      text,
      chip: true,
      title: `${head === null ? text : `${text} · ${head}`}\n${hint}`,
      spoken: spokenHead === null ? hint : `${spokenHead}, ${hint}`,
    };
  }
  const head = headText(s);
  if (head === null || head === "") return null;
  // the visible words say it all for a detached HEAD; a branch name is said as one
  return s.detached
    ? { text: head }
    : { text: head, branch: true, spoken: t("projects.node.srBranch", { branch: head }) };
}

/** The count of changed files as a number to show ("5000+" at the cap). */
export function changedCount(n: number): number | string {
  return n >= CHANGED_CAP ? `${CHANGED_CAP}+` : n;
}

/** Still being looked at: nothing counted yet while a look runs | is queued. */
export function checking(s: RepoSummary): boolean {
  return s.pending && s.changed === null;
}

/** The status of the project's Settings row: "develop · 3 changed" | "develop · clean" | "Checking git…" | the
 *  repository's trouble. Git missing: nothing (the intro says it once). */
export function rowFact(s: RepoSummary): ProjectFact | null {
  if (gitMissing(s)) return null;
  if (s.error?.kind === "failed") {
    const text = t("projects.card.gitError");
    return { text, title: `${text} ${reasonText(s.error.reason)}` };
  }
  const state = STATE_KEYS[s.state];
  const full = state === null ? headText(s) : t(state);
  // a long branch is cut so the count stays in sight (the tooltip has it whole); "not on a branch (at …)" is short
  const head = full === null ? null : s.detached ? full : cutEnd(full, ROW_HEAD_MAX);
  const parts = [head ?? ""].filter((p) => p !== "");
  const fullParts = [full ?? ""].filter((p) => p !== "");
  const tail = checking(s)
    ? t("projects.git.checking")
    : s.changed === null
      ? null
      : s.changed === 0
        ? t("projects.row.clean")
        : t("projects.row.changed", { n: changedCount(s.changed) });
  if (tail !== null) {
    parts.push(tail);
    fullParts.push(tail);
  }
  if (parts.length === 0) return null;
  const text = parts.join(" · ");
  const hint = stateHint(s.state);
  if (hint !== null) return { text, title: `${fullParts.join(" · ")}\n${hint}`, spoken: hint };
  const title = fullParts.join(" · ");
  return title === text ? { text } : { text, title };
}

/** Characters of a branch name the Settings row keeps before "…". */
export const ROW_HEAD_MAX = 24;

/** A text cut to `max` characters (whole characters, "…" at the end). */
export function cutEnd(text: string, max: number): string {
  const chars = Array.from(text);
  return chars.length <= max ? text : `${chars.slice(0, max - 1).join("")}…`;
}

/** The line of the expanded Settings row. */
export function infoText(s: RepoSummary | null, hasGit: boolean): string {
  if (!hasGit) return t("projects.rules.noGit");
  if (s === null) return t("projects.git.checking");
  if (gitMissing(s)) return t("projects.card.gitMissing");
  if (s.error?.kind === "failed") return t("projects.card.gitError");
  if (s.detached) return t("projects.rules.gitDetached", { oid: shortOid(s.oid) });
  const head = headText(s);
  return head === null ? t("git.status.noCommits") : t("projects.rules.gitRepo", { branch: head });
}

/** Ahead | behind in plain words: "1 change to send, 1 to get". */
export function aheadBehindText(ahead: number, behind: number): string {
  return `${tn("projects.card.toSend", ahead, { n: ahead })}, ${tn("projects.card.toGet", behind, { n: behind })}`;
}

/** Git's reason in words people know (dubious ownership -> "doesn't trust this folder's owner"). */
export function reasonText(reason: string): string {
  return /dubious ownership/i.test(reason) ? t("git.errorOwnership") : reason;
}

/** "N changed files" for the card. */
export function changedLine(n: number): string {
  return tn("projects.card.changed", n, { n: changedCount(n) });
}

/** Repositories by key of their folder (owner of a path = the longest prefix). */
export function reposByKey(repos: Iterable<RepoSummary>): ReadonlyMap<string, RepoSummary> {
  const map = new Map<string, RepoSummary>();
  for (const s of repos) map.set(keyOf(s.path), s);
  return map;
}

/** The repository of an included project that holds `path` (its folder included); `null`: the opened folder's own
 *  repository (or none). Worktree paths (`@wt/...`) belong to the opened folder. */
export function repoOf(
  byKey: ReadonlyMap<string, RepoSummary>,
  path: string | null,
): RepoSummary | null {
  if (path === null || byKey.size === 0 || path.startsWith("@wt/")) return null;
  let key = keyOf(path);
  for (;;) {
    const found = byKey.get(key);
    if (found) return found;
    const cut = key.lastIndexOf("/");
    if (cut <= 0) return null;
    key = key.slice(0, cut);
  }
}
