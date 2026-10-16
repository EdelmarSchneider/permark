import { afterEach, describe, expect, it } from "vitest";
import {
  gitBadgeTitle,
  gitExplanation,
  legendBaselineNote,
  legendRows,
  LEGEND_STATES,
} from "./gitLabels";
import { language } from "./i18n";
import { badgeFor } from "./tree";
import type { GitState } from "./types";

afterEach(() => {
  language.preference = "en";
});

const STATES: GitState[] = ["modified", "added", "untracked", "deleted", "renamed", "conflicted"];

describe("gitExplanation", () => {
  it("says what each state means in plain words", () => {
    expect(gitExplanation("modified")).toBe("Modified: this file changed since your last visit.");
    expect(gitExplanation("untracked")).toContain("Git doesn't know about yet");
    expect(gitExplanation("added")).toContain("next commit");
    for (const state of STATES) expect(gitExplanation(state).length).toBeGreaterThan(20);
  });

  it("names what the letters are compared with", () => {
    expect(gitExplanation("modified", "head")).toBe(
      "Modified: this file changed since the last Git commit.",
    );
    expect(gitExplanation("deleted", "lastVisit")).toContain("since your last visit");
    expect(gitExplanation("renamed", "head")).toContain("since the last Git commit");
  });
});

describe("gitBadgeTitle", () => {
  it("starts with the letter the tree draws and says nothing has to be committed", () => {
    for (const state of STATES) {
      const badge = badgeFor(state);
      if (!badge) throw new Error(state);
      const title = gitBadgeTitle(badge);
      expect(title.startsWith(`${badge.letter} · `)).toBe(true);
    }
    const title = gitBadgeTitle(badgeFor("modified")!);
    expect(title).toBe(
      "M · Modified: this file changed since your last visit. You don't have to commit anything: " +
        "the letter goes away once the change is committed, by you or your agent. TrailMark never commits.",
    );
  });

  it("tells a conflict what to do instead", () => {
    const title = gitBadgeTitle(badgeFor("conflicted")!);
    expect(title).toMatch(/^C · Conflict:/);
    expect(title).not.toContain("You don't have to commit");
  });

  it("is worded in the language of the interface", () => {
    language.preference = "pt-BR";
    expect(gitBadgeTitle(badgeFor("modified")!)).toMatch(/^M · /);
    expect(gitBadgeTitle(badgeFor("modified")!)).not.toContain("Modified");
    language.preference = "zh-CN";
    expect(gitBadgeTitle(badgeFor("untracked")!)).toMatch(/^U · /);
  });
});

describe("legendRows", () => {
  it("lists every letter the tree can show, once, in a fixed order", () => {
    const rows = legendRows();
    expect(rows.map((row) => row.badge.letter)).toEqual(["M", "A", "U", "D", "R", "C"]);
    expect(rows.map((row) => row.badge.state)).toEqual([...LEGEND_STATES]);
    expect(new Set(rows.map((row) => row.badge.className)).size).toBe(rows.length);
  });

  it("covers every git state", () => {
    expect(new Set(LEGEND_STATES)).toEqual(new Set(STATES));
  });

  it("follows what the letters are compared with", () => {
    expect(legendRows("head")[0].text).toContain("last Git commit");
    expect(legendRows("lastVisit")[0].text).toContain("last visit");
    expect(legendBaselineNote("head")).toContain("as soon as a commit is made");
    expect(legendBaselineNote("lastVisit")).toContain("next time you open this project");
  });
});
