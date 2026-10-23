// Repositories of included projects in the mock backend (`git.repos | interest | snapshot({ project })`, events
// `git:repos` + `git:projectSnapshot`; `projects.md` §8). Made from the window's projects view: every included row
// with a `.git` marker gets a repository whose facts depend on its place in the list, so fixtures show every case
// (branch, detached, merging, no commit yet, an error); details (ahead | behind, last commit) appear once a view
// asks for them, as in the app. The probe can make git missing or hold every answer ("Checking git…").

import type {
  BaselineKind,
  GitSnapshot,
  InterestLevel,
  PluginEvents,
  ProjectsView,
  RepoSummary,
} from "./types";

type Deps = {
  /** The projects view of the window's folder (`null`: plain folder). */
  view: () => ProjectsView | null;
  emit: <E extends keyof PluginEvents>(event: E, data: PluginEvents[E]) => void;
  /** "Changes since" of the window (shared by every repository, as in the app). */
  baseline: () => BaselineKind;
};

export type GitMembersProbe = {
  /** Interest calls, in order. */
  interest: { ids: string[]; level: InterestLevel }[];
  /** No git to ask: every repository says so, no branch anywhere. */
  unavailable(on: boolean): void;
  /** Every answer pending ("Checking git…") until released. */
  hold(on: boolean): void;
  /** The summaries now. */
  summaries(): RepoSummary[];
  /** Reshapes every summary (all dirty, mid-rebase, long branch names...); `null`: the fixture's facts again. */
  shape(fn: ((summary: RepoSummary, index: number) => RepoSummary) | null): void;
};

const HOUR = 3_600_000;

/** The facts of the repository at `index` (stable for a fixture). */
function factsOf(id: string, path: string, index: number, detail: boolean): RepoSummary {
  const detached = index % 6 === 3;
  const unborn = index % 11 === 8;
  const failed = index % 9 === 5;
  const branches = [
    "main",
    "develop",
    "feature/a-long-branch-name-that-truncates-first",
    "release/0.7",
  ];
  return {
    id,
    path,
    branch: detached ? null : (branches[index % branches.length] ?? "main"),
    detached,
    oid: unborn
      ? null
      : `${(index + 10).toString(16).padStart(2, "0")}a1b2c3d4e5f60718293a4b5c6d7e8f901234`.padEnd(
          40,
          "0",
        ),
    unborn,
    state: index % 7 === 4 ? "merging" : "clean",
    changed: failed ? null : (index * 3) % 7,
    aheadBehind: detail && !detached && index % 5 !== 2 ? [index % 3, index % 2] : null,
    lastCommit:
      detail && !unborn
        ? {
            atMs: Date.now() - (index + 1) * HOUR,
            subject: `Update the plan of ${path.split("/").pop() ?? path}`,
          }
        : null,
    error: failed ? { kind: "failed", reason: "detected dubious ownership in repository" } : null,
    pending: false,
  };
}

export function createMockGitMembers(deps: Deps): {
  repos(): RepoSummary[];
  interest(window: string, level: InterestLevel, ids: string[]): void;
  snapshot(project: string): GitSnapshot | null;
  refresh(project: string | null): void;
  /** "Changes since" changed: every list a view looks at is measured again and announced. */
  rebaseline(): void;
  probe: GitMembersProbe;
} {
  const interestCalls: { ids: string[]; level: InterestLevel }[] = [];
  const wants = new Map<string, Set<string>>();
  let unavailable = false;
  let held = false;
  let shaper: ((summary: RepoSummary, index: number) => RepoSummary) | null = null;
  let seq = 1_000;

  function rows(): { id: string; path: string }[] {
    const view = deps.view();
    if (!view) return [];
    return view.rows
      .filter(
        (r) =>
          r.state === "included" && r.uid !== null && (r.marker === "git" || r.marker === "both"),
      )
      .map((r) => ({ id: r.uid ?? "", path: r.path }));
  }

  function wanted(id: string, level?: InterestLevel): boolean {
    for (const [key, ids] of wants) {
      if (ids.has(id) && (!level || key.endsWith(`:${level}`))) return true;
    }
    return false;
  }

  function summaries(): RepoSummary[] {
    return rows().map(({ id, path }, index) => {
      const facts = factsOf(id, path, index, wanted(id, "detail"));
      const base: RepoSummary = shaper === null ? facts : shaper(facts, index);
      if (unavailable) {
        return {
          ...base,
          branch: null,
          detached: false,
          oid: null,
          unborn: false,
          state: "clean",
          changed: null,
          aheadBehind: null,
          lastCommit: null,
          error: { kind: "unavailable" },
        };
      }
      return held
        ? { ...base, changed: null, aheadBehind: null, lastCommit: null, pending: true }
        : base;
    });
  }

  function snapshotOf(id: string): GitSnapshot | null {
    const index = rows().findIndex((r) => r.id === id);
    const summary = summaries()[index];
    if (!summary || summary.changed === null || !wanted(id)) return null;
    const files = Array.from({ length: summary.changed }, (_, n) => ({
      path: `${summary.path}/${n === 0 ? "README.md" : `docs/note-${n}.md`}`,
      letter: n % 2 === 0 ? ("modified" as const) : ("untracked" as const),
      staged: false,
      from: null,
    }));
    return {
      seq,
      backend: "builtin",
      head: {
        branch: summary.branch,
        oid: summary.oid,
        detached: summary.detached,
        unborn: summary.unborn,
      },
      state: summary.state,
      baseline: { kind: deps.baseline(), oid: null },
      files,
      truncated: false,
      worktrees: [],
    };
  }

  function announce(ids: string[]): void {
    const all = summaries();
    const upsert = all.filter((s) => ids.includes(s.id));
    if (upsert.length > 0) deps.emit("git:repos", { upsert, removed: [] });
  }

  return {
    repos: summaries,
    interest(window, level, ids) {
      interestCalls.push({ ids: [...ids], level });
      const key = `${window}:${level}`;
      const before = wants.get(key) ?? new Set<string>();
      const known = new Set(rows().map((r) => r.id));
      const now = new Set(ids.filter((id) => known.has(id)));
      if (now.size === 0) wants.delete(key);
      else wants.set(key, now);
      const added = [...now].filter((id) => !before.has(id));
      if (added.length === 0) return;
      // the answer comes a moment later, as from the pool
      setTimeout(() => {
        seq += 1;
        announce(added);
        for (const id of added) {
          const snapshot = snapshotOf(id);
          if (snapshot) deps.emit("git:projectSnapshot", { project: id, snapshot });
        }
      }, 20);
    },
    snapshot: snapshotOf,
    refresh(project) {
      const ids = project
        ? [project]
        : rows()
            .map((r) => r.id)
            .filter((id) => wanted(id));
      setTimeout(() => announce(ids), 20);
    },
    rebaseline() {
      seq += 1;
      for (const { id } of rows()) {
        const snapshot = snapshotOf(id);
        if (snapshot) deps.emit("git:projectSnapshot", { project: id, snapshot });
      }
    },
    probe: {
      interest: interestCalls,
      unavailable(on) {
        unavailable = on;
        announce(rows().map((r) => r.id));
      },
      hold(on) {
        held = on;
        announce(rows().map((r) => r.id));
      },
      summaries,
      shape(fn) {
        shaper = fn;
        seq += 1;
        announce(rows().map((r) => r.id));
        for (const { id } of rows()) {
          const snapshot = snapshotOf(id);
          if (snapshot) deps.emit("git:projectSnapshot", { project: id, snapshot });
        }
      },
    },
  };
}
