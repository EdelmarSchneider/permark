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
import type { ReviewChange } from "./generated/ReviewChange";
import type {
  ActivityEntry,
  BaselineKind,
  FeedItem,
  GitSnapshot,
  PluginEventName,
  PluginEvents,
  PluginMethod,
  PluginMethods,
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
  /** Marks documents unread as if an agent changed them. */
  unread(paths: string[]): void;
  /** Calls the plugins got, `<plugin>.<method>`. */
  calls: string[];
  /** The git snapshot now. */
  git(): GitSnapshot;
  /** Announces a snapshot as the backend would (`null`: no repository any more). */
  announceGit(snapshot: GitSnapshot | null): void;
  /** Tags, comments, silence: what the organizing plugins hold. */
  organizing: OrganizingProbe;
  /** The notes: what the backend holds, ways to put them in a state. */
  notes: NotesProbe;
  /** The agent connection: what the window published, a client connecting, requests. */
  agent: AgentProbe;
  /** Announces a plugin event as the backend would. */
  announce<E extends PluginEventName>(event: E, data: PluginEvents[E]): void;
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

type MockReview = { changes: ReviewChange[]; undo: ReviewChange[][]; unit: "blocks" | "lines" };

export function createMockPlugins(
  emit: <E extends PluginEventName>(event: E, data: PluginEvents[E]) => void,
  hashOfGuide: () => string,
  organizing: ReturnType<typeof createMockOrganizing>,
  notes: ReturnType<typeof createMockNotes>,
  distribution: ReturnType<typeof createMockDistribution>["handlers"] &
    ReturnType<typeof createMockTokens>["handlers"] &
    ReturnType<typeof createMockConvert>["handlers"],
): {
  call<M extends PluginMethod>(
    method: M,
    args: PluginMethods[M]["args"],
  ): Promise<Result<PluginMethods[M]["result"]>>;
  probe: MockPluginProbe;
} {
  const calls: string[] = [];
  let baseline: BaselineKind = "lastVisit";
  let basis: PluginMethods["review.basis"]["result"] = "read";
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
      oid: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
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
  let unread = new Set<string>([GUIDE]);
  const unreadList = (): UnreadList => ({ ready: true, unread: [...unread].sort() });
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
    [GUIDE, { changes: startChanges(), undo: [], unit: "blocks" }],
    [CODE, { changes: startCodeChanges(), undo: [], unit: "lines" }],
  ]);
  /** What a document has against the last commit when there is nothing new against what was read. */
  const againstCommit = (path: string, asked: typeof basis): ReviewChange[] => {
    const review = reviews.get(path);
    if (asked !== "commit" || !review || review.changes.length > 0 || path !== CODE) {
      return review?.changes ?? [];
    }
    return startCodeChanges().slice(0, 1);
  };
  const reviewView = (
    path: string,
    asked: typeof basis = basis,
  ): PluginMethods["review.open"]["result"] => {
    const review = reviews.get(path);
    const changes = againstCommit(path, asked);
    return {
      path,
      hash: path === GUIDE ? hashOfGuide() : "0000000000000000",
      basis: asked,
      asked,
      unit: review?.unit ?? "blocks",
      count: changes.length,
      canKeep: asked === "read",
      changes: changes.map((c, index) => ({ ...c, index })),
    };
  };
  /** Takes the changes of a review through `change` (the old ones can be put back by Undo). */
  const alter = (path: string, change: (changes: ReviewChange[]) => ReviewChange[]): void => {
    const review = reviews.get(path);
    if (!review) return;
    review.undo.push(review.changes);
    review.changes = change(review.changes);
  };

  const agent = createMockAgent(emit);
  const handlers: Handlers = {
    ...organizing.handlers,
    ...agent.handlers,
    ...notes.handlers,
    ...distribution,
    "git.snapshot": () => snapshot(),
    "git.refresh": () => null,
    "git.setBaseline": ({ kind }) => {
      baseline = kind;
      seq += 1;
      emit("git:snapshot", snapshot());
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
      alter(path, (all) => all.filter((_, i) => i !== index));
      return reviewView(path);
    },
    "review.keepAll": ({ path }) => {
      alter(path, () => []);
      return reviewView(path);
    },
    "review.revert": ({ path, index }) => {
      alter(path, (all) => all.filter((_, i) => i !== index));
      return reviewView(path);
    },
    "review.revertAll": ({ path }) => {
      alter(path, () => []);
      return reviewView(path);
    },
    "review.undo": ({ path }) => {
      const review = reviews.get(path);
      if (review) review.changes = review.undo.pop() ?? review.changes;
      return reviewView(path);
    },
    "review.release": () => null,
    "review.restore": () => null,
    "review.setBasis": (args) => {
      basis = args.basis;
      return null;
    },
    "review.basis": () => basis,
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
            error: { kind: error.kind, subject: error.subject, detail: null },
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
        for (const path of paths) unread.add(path);
        announceUnread();
      },
      calls,
      git: snapshot,
      announceGit: (next) => emit("git:snapshot", next),
      organizing: organizing.probe,
      notes: notes.probe,
      agent: agent.probe,
      announce: emit,
    },
  };
}
