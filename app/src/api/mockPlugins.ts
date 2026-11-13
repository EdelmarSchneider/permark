// What the plugins' backend halves answer in the mock backend (`dev:mock`, tests): git letters on two files, a feed,
// unread documents, and a review of `docs/guide.md`. In memory; the same shapes the real plugins serve.

import type { Block } from "./generated/Block";
import { createMockAgent, type AgentProbe } from "./mockAgent";
import { MockPluginError, type OrganizingProbe } from "./mockOrganizing";
import type { createMockOrganizing } from "./mockOrganizing";
import type { createMockNotes, NotesProbe } from "./mockNotes";
import type { createMockDistribution } from "./mockDistribution";
import type { createMockTokens } from "./mockTokens";
import type { createMockConvert } from "./mockConvert";
import type { createMockCorrection } from "./mockCorrection";
import type { createMockTranslation } from "./mockTranslation";
import type { createMockLocalAi } from "./mockLocalAi";
import type { ReviewChange } from "./generated/ReviewChange";
import { createMockGitMembers, type GitMembersProbe } from "./mockGitMembers";
import {
  MOCK_HEAD,
  mockVersionChanges,
  mockVersions,
  type MockFileState,
} from "./mockReviewVersions";
import type {
  ActivityEntry,
  BaselineKind,
  Basis,
  FeedItem,
  GitSnapshot,
  PluginEventName,
  PluginEvents,
  PluginMethod,
  PluginMethods,
  ProjectsView,
  RepairArgs,
  RepairRefusal,
  Result,
  SinceVisit,
  UnreadList,
} from "./types";

const GUIDE = "docs/guide.md";

type Handlers = {
  [M in PluginMethod]: (args: PluginMethods[M]["args"]) => PluginMethods[M]["result"];
};

export type MockPluginProbe = {
  /** Pushes feed items as if the watch had reported them. */
  feed(items: FeedItem[]): void;
  /** An action of the person as if the activity journal had noted it. */
  mine(entry: ActivityEntry): void;
  /** Marks documents unread as if an agent changed them, in this order (the last one is the newest change). */
  unread(paths: string[]): void;
  /** Calls the plugins got, `<plugin>.<method>`. */
  calls: string[];
  /** The git snapshot now. */
  git(): GitSnapshot;
  /** Announces a snapshot as the backend would (`null`: no repository any more). */
  announceGit(snapshot: GitSnapshot | null): void;
  /** Repositories of included projects (`git.repos`, interest, member snapshots). */
  gitMembers: GitMembersProbe;
  /** Tags, comments, silence: what the organizing plugins hold. */
  organizing: OrganizingProbe;
  /** The notes: what the backend holds, ways to put them in a state. */
  notes: NotesProbe;
  /** The agent connection: what the window published, a client connecting, requests. */
  agent: AgentProbe;
  /** Announces a plugin event as the backend would. */
  announce<E extends PluginEventName>(event: E, data: PluginEvents[E]): void;
  /** Repair calls, oldest first. */
  repairs: RepairArgs[];
  /** The next repair returns this refusal and writes nothing (`null` clears one still waiting). */
  refuseNext(reason: RepairRefusal | null): void;
};

function ghost(id: string, html: string): Block {
  return { id, kind: "p", lineStart: 1, lineEnd: 1, html: `<p data-b="${id}">${html}</p>` };
}

function startChanges(): ReviewChange[] {
  return [
    {
      index: 0,
      kind: "edited",
      start: 3,
      len: 1,
      ghosts: [ghost("old-open", "Open a folder and read it.")],
      old: [],
      whitespace: false,
    },
    {
      index: 1,
      kind: "removed",
      start: 5,
      len: 0,
      ghosts: [ghost("old-gone", "This paragraph was removed by the agent.")],
      old: [],
      whitespace: false,
    },
  ];
}

const CODE = "src/main.rs";

/** What an agent did to the mock code file: a line replaced, a comment taken out. */
function startCodeChanges(): ReviewChange[] {
  return [
    {
      index: 0,
      kind: "edited",
      start: 1,
      len: 1,
      ghosts: [],
      old: ['    println!("hi");'],
      whitespace: false,
    },
    {
      index: 1,
      kind: "removed",
      start: 2,
      len: 0,
      ghosts: [],
      old: ["    // TODO: remove"],
      whitespace: false,
    },
  ];
}

/** The changes of one document against one basis. */
type Drawn = { changes: ReviewChange[] };
type MockReview = Drawn & { unit: "blocks" | "lines" };

export function createMockPlugins(
  emit: <E extends PluginEventName>(event: E, data: PluginEvents[E]) => void,
  hashOfGuide: () => string,
  organizing: ReturnType<typeof createMockOrganizing>,
  notes: ReturnType<typeof createMockNotes>,
  distribution: ReturnType<typeof createMockDistribution>["handlers"] &
    ReturnType<typeof createMockTokens>["handlers"] &
    ReturnType<typeof createMockConvert>["handlers"] &
    ReturnType<typeof createMockLocalAi>["handlers"] &
    ReturnType<typeof createMockCorrection>["handlers"] &
    ReturnType<typeof createMockTranslation>["handlers"],
  projectsView: () => ProjectsView | null = () => null,
): {
  call<M extends PluginMethod>(
    method: M,
    args: PluginMethods[M]["args"],
  ): Promise<Result<PluginMethods[M]["result"]>>;
  probe: MockPluginProbe;
} {
  const calls: string[] = [];
  let baseline: BaselineKind = "lastVisit";
  /** The choice the backend remembers (a version never is: it comes with each call). */
  let basis: Exclude<Basis, { version: string }> = "read";
  const gitFiles: GitSnapshot["files"] = [
    { path: GUIDE, letter: "modified", staged: false, from: null },
    { path: "notes/ideas.md", letter: "untracked", staged: false, from: null },
  ];
  let seq = 1;
  const snapshot = (): GitSnapshot => ({
    seq,
    backend: "builtin",
    head: {
      branch: "main",
      oid: MOCK_HEAD,
      detached: false,
      unborn: false,
    },
    state: "clean",
    baseline: {
      kind: baseline,
      oid: baseline === "lastVisit" ? "0f1e2d3c4b5a69788796a5b4c3d2e1f001122334" : null,
    },
    files: gitFiles,
    truncated: false,
    worktrees: [],
  });
  // the person's own actions (the activity journal), oldest first, with a hand-off between them
  let mine: ActivityEntry[] = [
    {
      seq: 1,
      at: Date.now() - 3_600_000,
      by: "person",
      fact: { kind: "tag", added: true, path: GUIDE, tag: "plan" },
    },
    { seq: 2, at: Date.now() - 3_000_000, by: "person", fact: { kind: "handOff", rows: 3 } },
    {
      seq: 3,
      at: Date.now() - 60_000,
      by: "person",
      fact: { kind: "save", path: GUIDE, sections: ["Guide > Install"], blocks: 2 },
    },
    {
      seq: 4,
      at: Date.now() - 45_000,
      by: "person",
      fact: { kind: "comment", op: "add", path: GUIDE, id: "c9", heading: "Guide" },
    },
  ];
  let feed: FeedItem[] = [
    {
      item: "entry",
      id: 2,
      path: GUIDE,
      kind: "modified",
      atMs: Date.now() - 20_000,
      saves: 3,
      own: false,
      isDir: false,
    },
    {
      item: "entry",
      id: 1,
      path: "notes/ideas.md",
      kind: "created",
      atMs: Date.now() - 90_000,
      saves: 1,
      own: false,
      isDir: false,
    },
  ];
  /** Unread documents, oldest change first (the backend sends the newest first). */
  let unread = new Set<string>([GUIDE]);
  const changed = (path: string): void => {
    unread.delete(path);
    unread.add(path);
  };
  const unreadList = (): UnreadList => ({ ready: true, unread: [...unread].reverse() });
  const announceUnread = (): void => emit("unread:changed", unreadList());
  const since: SinceVisit = {
    ready: true,
    first: false,
    atMs: Date.now() - 3_600_000,
    items: [
      { path: GUIDE, kind: "changed" },
      { path: "notes/ideas.md", kind: "new" },
    ],
    more: 0,
  };
  const reviews = new Map<string, MockReview>([
    [GUIDE, { changes: startChanges(), unit: "blocks" }],
    [CODE, { changes: startCodeChanges(), unit: "lines" }],
  ]);
  /** What a document has against the last commit when there is nothing new against what was read. */
  const againstCommit = (path: string, asked: typeof basis): ReviewChange[] => {
    const review = reviews.get(path);
    if (asked !== "commit" || !review || review.changes.length > 0 || path !== CODE) {
      return review?.changes ?? [];
    }
    return startCodeChanges().slice(0, 1);
  };
  const unitOf = (path: string): MockReview["unit"] =>
    reviews.get(path)?.unit ?? (/\.(md|markdown)$/i.test(path) ? "blocks" : "lines");
  /** How a file stands toward the last commit (what the git letters and the review say). */
  const fileState = (path: string): MockFileState => {
    const letter = gitFiles.find((file) => file.path === path)?.letter;
    if (letter === "untracked") return "untracked";
    return letter !== undefined || againstCommit(path, "commit").length > 0
      ? "changed"
      : "unchanged";
  };
  /** What documents have against the versions asked for so far (`<commit> <path>`): made at the first ask from
   *  what the file has against the last commit and what the commits since the version changed. */
  const versioned = new Map<string, Drawn>();
  /** The changes against `asked`, held so that a revert can take some out; `null`: a version that cannot be had. */
  const drawn = (path: string, asked: Basis): Drawn | null => {
    if (typeof asked === "string") return reviews.get(path) ?? null;
    const key = `${asked.version} ${path}`;
    const known = versioned.get(key);
    if (known) return known;
    const since = mockVersionChanges(path, asked.version, unitOf(path));
    if (since === null) return null;
    const made = { changes: [...againstCommit(path, "commit"), ...since] };
    versioned.set(key, made);
    return made;
  };
  /** The basis a call goes against: the version it names, else the remembered choice. */
  const held = (asked: Basis | undefined): Basis => (typeof asked === "object" ? asked : basis);
  const reviewView = (
    path: string,
    asked: Basis = basis,
  ): PluginMethods["review.open"]["result"] => {
    const version = typeof asked === "object" ? drawn(path, asked) : null;
    if (typeof asked === "object" && version === null) {
      throw new MockPluginError("unavailable", "noBasis");
    }
    const changes =
      version?.changes ?? againstCommit(path, typeof asked === "string" ? asked : basis);
    return {
      path,
      hash: path === GUIDE ? hashOfGuide() : "0000000000000000",
      basis: asked,
      asked,
      unit: unitOf(path),
      count: changes.length,
      canKeep: asked === "read",
      uncommitted: typeof asked === "object" && fileState(path) === "changed",
      changes: changes.map((c, index) => ({ ...c, index })),
    };
  };
  /** What Undo puts back, per document: the newest last. */
  const undos = new Map<string, (() => void)[]>();
  /** Takes the changes a document has against `asked` through `change` (Undo puts the old ones back). */
  const alter = (
    path: string,
    asked: Basis,
    change: (changes: ReviewChange[]) => ReviewChange[],
  ): void => {
    const target = drawn(path, asked);
    if (!target) return;
    const before = target.changes;
    undos.set(path, [...(undos.get(path) ?? []), () => (target.changes = before)]);
    target.changes = change(before);
  };

  const agent = createMockAgent(emit);
  const members = createMockGitMembers({ view: projectsView, emit, baseline: () => baseline });
  const repairs: RepairArgs[] = [];
  let nextRefusal: RepairRefusal | null = null;
  const handlers: Handlers = {
    ...organizing.handlers,
    ...agent.handlers,
    ...notes.handlers,
    ...distribution,
    "git.snapshot": ({ project }) => (project ? members.snapshot(project) : snapshot()),
    "git.refresh": ({ project }) => {
      members.refresh(project ?? null);
      return null;
    },
    "git.repos": () => members.repos(),
    "git.interest": ({ ids, level, view }) => {
      members.interest(view ?? "main", level, ids);
      return null;
    },
    "git.setBaseline": ({ kind }) => {
      baseline = kind;
      seq += 1;
      emit("git:snapshot", snapshot());
      members.rebaseline();
      return null;
    },
    "live-feed.items": () => feed,
    "live-feed.clear": () => {
      feed = [];
      return null;
    },
    "live-feed.activity": () => mine,
    "unread.list": () => unreadList(),
    "unread.sinceLastVisit": () => since,
    "unread.markRead": ({ path }) => {
      unread.delete(path);
      announceUnread();
      return null;
    },
    "unread.markUnread": ({ path }) => {
      unread.add(path);
      announceUnread();
      return null;
    },
    "unread.markFolderRead": ({ folder }) => {
      unread = new Set(
        [...unread].filter((p) => folder !== "" && !(p === folder || p.startsWith(`${folder}/`))),
      );
      announceUnread();
      return null;
    },
    "review.seen": () => null,
    "review.status": ({ path, basis: asked = basis }) => {
      if (typeof asked === "object") {
        const count = drawn(path, asked)?.changes.length;
        if (count === undefined) {
          return { state: { state: "noBasis" }, basis: asked, gap: "missing" };
        }
        return { state: count > 0 ? { state: "changes", count } : { state: "same" }, basis: asked };
      }
      const review = reviews.get(path);
      const count = againstCommit(path, asked).length;
      const state: PluginMethods["review.status"]["result"]["state"] = review
        ? count > 0
          ? { state: "changes", count }
          : { state: "same" }
        : /\.md$/i.test(path)
          ? { state: "same" }
          : { state: "noBasis" };
      return { state, basis: asked };
    },
    "review.open": ({ path, basis: asked }) => reviewView(path, asked),
    "review.keep": ({ path, index }) => {
      alter(path, basis, (all) => all.filter((_, i) => i !== index));
      return reviewView(path);
    },
    "review.keepAll": ({ path }) => {
      alter(path, basis, () => []);
      return reviewView(path);
    },
    "review.revert": ({ path, index, basis: asked }) => {
      alter(path, held(asked), (all) => all.filter((_, i) => i !== index));
      return reviewView(path, held(asked));
    },
    "review.revertAll": ({ path, basis: asked }) => {
      alter(path, held(asked), () => []);
      return reviewView(path, held(asked));
    },
    "review.undo": ({ path, basis: asked }) => {
      undos.get(path)?.pop()?.();
      return reviewView(path, held(asked));
    },
    "review.release": () => null,
    "review.restore": () => null,
    "review.setBasis": (args) => {
      // a version is for one review: it is not remembered
      if (typeof args.basis === "string") basis = args.basis;
      return null;
    },
    "review.basis": () => basis,
    "review.versions": ({ path, after }) => mockVersions(path, after ?? 0, fileState(path)),
    "broken-links.repair": (args) => {
      repairs.push(args);
      const reason = nextRefusal;
      nextRefusal = null;
      if (reason !== null) return { outcome: "refused", reason };
      return { outcome: "wrote", href: args.target };
    },
  };

  return {
    call: (method, args) => {
      calls.push(method);
      try {
        return Promise.resolve({ ok: true, value: handlers[method](args) });
      } catch (error) {
        if (!(error instanceof MockPluginError)) throw error;
        return Promise.resolve({
          ok: false,
          error: {
            kind: "backend",
            command: "plugin_call",
            error: { kind: error.kind, subject: error.subject, detail: error.detail },
          },
        });
      }
    },
    probe: {
      feed: (items) => {
        feed = [...items, ...feed];
        emit("live-feed:items", { upsert: items });
      },
      mine: (entry) => {
        mine = [...mine, entry];
        emit("live-feed:activity", { entry });
      },
      unread: (paths) => {
        for (const path of paths) changed(path);
        announceUnread();
      },
      calls,
      git: snapshot,
      announceGit: (next) => emit("git:snapshot", next),
      gitMembers: members.probe,
      organizing: organizing.probe,
      notes: notes.probe,
      agent: agent.probe,
      announce: emit,
      repairs,
      refuseNext: (reason) => {
        nextRefusal = reason;
      },
    },
  };
}
