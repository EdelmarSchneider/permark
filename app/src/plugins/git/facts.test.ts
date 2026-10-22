// What the git plugin says about a project's repository (`projects.md` §12.2-12.4): node text, Settings row, expanded
// row line, owner repository of a path. Pure.
import { describe, expect, it } from "vitest";
import type { RepoSummary } from "../../api";
import type { RepoState } from "../../api/generated/RepoState";
import {
  CHANGED_CAP,
  STATE_KEYS,
  aheadBehindText,
  changedLine,
  infoText,
  nodeFact,
  reasonText,
  repoOf,
  reposByKey,
  rowFact,
  stateHint,
} from "./facts";

function repo(patch: Partial<RepoSummary> = {}): RepoSummary {
  return {
    id: "uid0000001",
    path: "hubs/hub-b",
    branch: "develop",
    detached: false,
    oid: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
    unborn: false,
    state: "clean",
    changed: 3,
    aheadBehind: null,
    lastCommit: null,
    error: null,
    pending: false,
    ...patch,
  };
}

describe("the node's muted text", () => {
  it("is the branch with a branch mark, spoken as 'branch …'", () => {
    expect(nodeFact(repo())).toEqual({ text: "develop", branch: true, spoken: "branch develop" });
  });

  it("a detached HEAD in plain words + 7 characters of the commit (nothing more to say)", () => {
    expect(nodeFact(repo({ branch: null, detached: true }))).toEqual({
      text: "not on a branch (at a1b2c3d)",
    });
  });

  it("what the repository is doing replaces the branch, as a chip (mid-rebase: detached under it)", () => {
    const fact = nodeFact(repo({ state: "rebasing", branch: null, detached: true }));
    expect(fact).toMatchObject({ text: "REBASING", chip: true });
    // the chip's meaning in plain words: in the tooltip and said (a detached HEAD: the visible words say it)
    expect(fact?.title).toBe(
      "REBASING · not on a branch (at a1b2c3d)\nA rebase was started in git and isn't finished yet.",
    );
    expect(fact?.spoken).toBe("A rebase was started in git and isn't finished yet.");
    expect(nodeFact(repo({ state: "merging" }))).toEqual({
      text: "MERGING",
      chip: true,
      title: "MERGING · develop\nA merge was started in git and isn't finished yet.",
      spoken: "branch develop, A merge was started in git and isn't finished yet.",
    });
  });

  it("every state chip has its meaning in words; clean has none", () => {
    for (const state of Object.keys(STATE_KEYS) as RepoState[]) {
      expect(stateHint(state) === null).toBe(state === "clean");
    }
  });

  it("no commit yet: the branch it is on (UX F12); no name: nothing; git missing: nothing anywhere", () => {
    expect(nodeFact(repo({ unborn: true, oid: null }))).toMatchObject({
      text: "develop",
      branch: true,
    });
    expect(nodeFact(repo({ unborn: true, oid: null, branch: null }))).toBeNull();
    expect(nodeFact(repo({ error: { kind: "unavailable" }, branch: null }))).toBeNull();
    // git missing wins over a branch the cheap reader still had
    expect(nodeFact(repo({ error: { kind: "unavailable" } }))).toBeNull();
  });

  it("long and Unicode branch names come through whole (the view cuts them)", () => {
    const long = `feature/${"x".repeat(200)}`;
    expect(nodeFact(repo({ branch: long }))?.text).toBe(long);
    expect(nodeFact(repo({ branch: "功能/数据-ünïcödé" }))?.text).toBe("功能/数据-ünïcödé");
  });

  it("a repository git can't read: a warning, said and in the tooltip; the branch it read without git stays", () => {
    expect(
      nodeFact(repo({ error: { kind: "failed", reason: "dubious ownership" }, changed: null })),
    ).toEqual({
      text: "develop",
      warn: true,
      spoken: "Git can't read this project.",
      title: "Git can't read this project.",
    });
    // nothing read at all: the mark alone
    expect(
      nodeFact(
        repo({ error: { kind: "failed", reason: "x" }, branch: null, oid: null, changed: null }),
      ),
    ).toMatchObject({ text: "", warn: true });
  });
});

describe("the Settings row", () => {
  it("branch · n changed | branch · clean", () => {
    expect(rowFact(repo())?.text).toBe("develop · 3 changed");
    expect(rowFact(repo({ changed: 0 }))?.text).toBe("develop · clean");
  });

  it("Checking git… while the first count runs (not once a count is known)", () => {
    expect(rowFact(repo({ pending: true, changed: null }))?.text).toBe("develop · Checking git…");
    expect(rowFact(repo({ pending: true, changed: 2 }))?.text).toBe("develop · 2 changed");
    expect(rowFact(repo({ pending: true, changed: null, branch: null, unborn: true }))?.text).toBe(
      "Checking git…",
    );
  });

  it("a long branch is cut so the count stays; the tooltip has it whole", () => {
    const long = `feature/${"a".repeat(60)}`;
    const fact = rowFact(repo({ branch: long, changed: 4 }));
    expect(fact?.text).toBe(`${long.slice(0, 23)}… · 4 changed`);
    expect(fact?.title).toBe(`${long} · 4 changed`);
    // whole characters: a Unicode name is never cut inside one
    const wide = "功能/数据-ünïcödé-✨✨✨✨✨✨✨✨✨✨✨✨✨✨✨✨✨✨";
    expect(Array.from(rowFact(repo({ branch: wide }))?.text.split(" · ")[0] ?? "")).toHaveLength(
      24,
    );
  });

  it("the cap reads 5000+", () => {
    expect(rowFact(repo({ changed: CHANGED_CAP }))?.text).toBe("develop · 5000+ changed");
    expect(changedLine(CHANGED_CAP)).toBe("5000+ changed files");
    expect(changedLine(1)).toBe("1 changed file");
  });

  it("a state replaces the branch; trouble says so; git missing says nothing (the intro does)", () => {
    const merging = rowFact(repo({ state: "merging", changed: 1 }));
    expect(merging).toEqual({
      text: "MERGING · 1 changed",
      title: "MERGING · 1 changed\nA merge was started in git and isn't finished yet.",
      spoken: "A merge was started in git and isn't finished yet.",
    });
    const failed = rowFact(
      repo({ error: { kind: "failed", reason: "detected dubious ownership" } }),
    );
    expect(failed?.text).toBe("Git can't read this project.");
    // git's own message in plain words, as on the card
    expect(failed?.title).toBe(
      "Git can't read this project. Git doesn't trust this folder's owner.",
    );
    // a detached HEAD is never cut (short and whole)
    expect(rowFact(repo({ branch: null, detached: true, changed: 2 }))?.text).toBe(
      "not on a branch (at a1b2c3d) · 2 changed",
    );
    expect(rowFact(repo({ error: { kind: "unavailable" } }))).toBeNull();
  });
});

describe("the expanded row's line", () => {
  it("its own repository, branch …; no git; git missing; can't read", () => {
    expect(infoText(repo(), true)).toBe("Its own repository, branch develop.");
    expect(infoText(repo({ branch: null, detached: true }), true)).toBe(
      "Its own repository, not on a branch (at a1b2c3d).",
    );
    expect(infoText(null, false)).toBe("No git repository.");
    expect(infoText(null, true)).toBe("Checking git…");
    expect(infoText(repo({ error: { kind: "unavailable" } }), true)).toBe(
      "Git isn't available, so branch and changes aren't shown.",
    );
    expect(infoText(repo({ error: { kind: "failed", reason: "x" } }), true)).toBe(
      "Git can't read this project.",
    );
  });

  it("ahead | behind in plain words, singular and plural", () => {
    expect(aheadBehindText(1, 1)).toBe("1 change to send, 1 to get");
    expect(aheadBehindText(3, 2)).toBe("3 changes to send, 2 to get");
  });

  it("git's ownership refusal in plain words", () => {
    expect(reasonText("fatal: detected dubious ownership in repository at 'x'")).toBe(
      "Git doesn't trust this folder's owner.",
    );
    expect(reasonText("bad object")).toBe("bad object");
  });
});

describe("the repository of a path", () => {
  const repos = reposByKey([
    repo({ id: "outer00001", path: "hubs/hub-b" }),
    repo({ id: "inner00001", path: "hubs/hub-b/projects/spec" }),
    repo({ id: "cafe000001", path: "repos/Café NFD".normalize("NFD") }),
  ]);

  it("the longest folder holding it; a sibling with the same start is not inside", () => {
    expect(repoOf(repos, "hubs/hub-b/README.md")?.id).toBe("outer00001");
    expect(repoOf(repos, "hubs/hub-b/projects/spec/plan.md")?.id).toBe("inner00001");
    expect(repoOf(repos, "hubs/hub-b")?.id).toBe("outer00001");
    expect(repoOf(repos, "hubs/hub-bc/README.md")).toBeNull();
    expect(repoOf(repos, "README.md")).toBeNull();
  });

  it("case and NFC | NFD spellings of one folder match; worktrees and no path belong to the opened folder", () => {
    expect(repoOf(repos, "HUBS/Hub-B/x.md")?.id).toBe("outer00001");
    expect(repoOf(repos, "repos/Café NFD/a.md".normalize("NFC"))?.id).toBe("cafe000001");
    expect(repoOf(repos, "@wt/feature/hubs/hub-b/x.md")).toBeNull();
    expect(repoOf(repos, null)).toBeNull();
    expect(repoOf(new Map(), "hubs/hub-b/x.md")).toBeNull();
  });
});
