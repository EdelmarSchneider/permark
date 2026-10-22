// Projects inside a folder for the mock backend (dev:mock, tests, the UX probe). The fixture is chosen by the folder a
// tab opens (seed `project path=<folder>`): the mock's own folder stays plain (nothing new, acceptance A0). Shapes
// mirror the backend (`projects.md`); the rules live in Rust with their tests, the mock only plays them.

import type {
  Api,
  CardDto,
  CardTarget,
  Events,
  FeedItem,
  MoveAsk,
  ProjectEvent,
  ProjectPick,
  ProjectRow,
  ProjectsView,
  Result,
  TagStatus,
  VocabularyEntry,
  VocabularyKind,
} from "./types";
import { COLORS, ICONS } from "../plugins/tags/model";

const ok = <T>(value: T): Promise<Result<T>> => Promise.resolve({ ok: true, value });

/** Folder holding projects, nothing answered yet: the first question (an inner root brings one project of its own). */
export const MOCK_PROJECTS_FRESH = "C:/Users/demo/workspace";
/** Folder with a project list: included, nested, missing, declined, one new project. */
export const MOCK_PROJECTS_LISTED = "C:/Users/demo/workspace-listed";
/** Folder whose project list cannot be read (line 3). */
export const MOCK_PROJECTS_BROKEN = "C:/Users/demo/workspace-broken";
/** A plain folder holding one cloned repository (A14). */
export const MOCK_PROJECTS_ONE = "C:/Users/demo/template";
/** A large folder (like a monorepo of repos): 36 included (long + Unicode names), 3 new, 1 missing. */
export const MOCK_PROJECTS_MANY = "C:/Users/demo/workspace-many";
/** Official tags + categories in the folder's `.permark/vocabulary.toml`; projects with lists of their own (round 4). */
export const MOCK_PROJECTS_VOCAB = "C:/Users/demo/workspace-vocab";

/** The lists included projects keep in their own `.permark/vocabulary.toml` (local to them), per fixture folder. */
const LOCAL_VOCAB: Record<string, Record<string, { tags: string[]; categories: string[] }>> = {
  [MOCK_PROJECTS_VOCAB]: {
    "hub-b": { tags: ["draft", "spec"], categories: [] },
    site: { tags: [], categories: ["website"] },
  },
};

/**
 * Official | local | unlisted of a tag | category (`projects.md` §7.2, the backend's rule): official = in the folder's
 * list; local = in the list of an included project holding `path` (innermost first; `path` null = any project);
 * else unlisted. Dormant (no official list): nulls.
 */
export function mockTagStatus(
  v: ProjectsView | null,
  root: string,
  kind: VocabularyKind,
  name: string,
  path: string | null,
): { status: TagStatus | null; owner: string | null } {
  if (!v?.vocabulary.active) return { status: null, owner: null };
  const official = kind === "tag" ? v.vocabulary.tags : v.vocabulary.categories;
  if (official.some((e) => e.name === name)) return { status: "official", owner: null };
  const lists = LOCAL_VOCAB[root] ?? {};
  const chain = v.rows
    .filter((r) => r.state === "included" && r.uid !== null)
    .filter((r) => path === null || path === r.path || path.startsWith(`${r.path}/`))
    .sort((a, b) => b.path.length - a.path.length);
  for (const r of chain) {
    const own = lists[r.path];
    if (own && (kind === "tag" ? own.tags : own.categories).includes(name)) {
      return { status: "local", owner: r.uid };
    }
  }
  return { status: "unlisted", owner: null };
}

/** Category marks + local list counts of a view (what the backend fills in). */
function marked(root: string, v: ProjectsView): ProjectsView {
  const lists = LOCAL_VOCAB[root] ?? {};
  const official = (kind: VocabularyKind): string[] =>
    (kind === "tag" ? v.vocabulary.tags : v.vocabulary.categories).map((e) => e.name);
  return {
    ...v,
    rootCategoryStatus:
      v.rootCard.category === null
        ? null
        : mockTagStatus(v, root, "category", v.rootCard.category, "").status,
    rows: v.rows.map((r) => {
      if (r.state !== "included") return r;
      const own = lists[r.path] ?? { tags: [], categories: [] };
      return {
        ...r,
        categoryStatus:
          r.category === null
            ? null
            : mockTagStatus(v, root, "category", r.category, r.path).status,
        localTags: own.tags.filter((t) => !official("tag").includes(t)).length,
        localCategories: own.categories.filter((c) => !official("category").includes(c)).length,
      };
    }),
  };
}

/** Folder names of the large fixture: plain, long, Unicode (CJK, accents, NFD), spaces. */
const MANY_NAMES: readonly string[] = [
  ...Array.from({ length: 30 }, (_, n) => `repo-${String(n + 1).padStart(2, "0")}`),
  "a-very-long-project-folder-name-that-keeps-going-past-the-sidebar-width-and-then-some-more",
  "数据分析项目",
  "Ünïcödé café",
  "Café NFD",
  "space name here",
  "émoji-✨-notes",
];

type Fixture = {
  files: Record<string, string>;
  view: () => ProjectsView;
};

/** A uid of the backend's alphabet (`[a-z2-7]`, 10 characters). */
function mockUid(n: number): string {
  const letters = "abcdefghijklmnopqrstuvwxyz";
  return `mockuid${letters[n % 26]}${letters[Math.floor(n / 26) % 26]}a`;
}

function nameOf(path: string): string {
  return path.split("/").pop() ?? path;
}

function card(name: string, place: CardDto["place"] = "profile"): CardDto {
  return {
    name,
    short: name,
    description: null,
    category: null,
    place,
    invalid: [],
    warning: null,
  };
}

function row(path: string, fields: Partial<ProjectRow> = {}): ProjectRow {
  const name = nameOf(path);
  return {
    key: path.toLowerCase(),
    uid: null,
    path,
    state: "offered",
    missing: null,
    marker: "git",
    name,
    short: name,
    description: null,
    category: null,
    hidden: "own",
    hiddenUnknown: false,
    agent: "same",
    parent: null,
    inner: null,
    cardPlace: null,
    locateFrom: null,
    categoryStatus: null,
    localTags: 0,
    localCategories: 0,
    ...fields,
  };
}

/** The strip summary counted from the rows. */
function summarized(v: ProjectsView): ProjectsView {
  const offered = v.rows.filter((r) => r.state === "offered");
  return {
    ...v,
    present: v.present || v.rows.length > 0,
    offered: {
      count: offered.length,
      names: offered.slice(0, 3).map((r) => r.name),
      inner: offered
        .filter((r) => r.inner !== null)
        .map((r) => ({ name: r.name, count: r.inner ?? 0 })),
    },
  };
}

/** Where Locate… opens for a missing row: its old parent folder (the mock keeps every parent), else the root. */
function located(root: string, r: ProjectRow): ProjectRow {
  if (r.state !== "missing") return r;
  const cut = r.path.lastIndexOf("/");
  return { ...r, locateFrom: cut > 0 ? `${root}/${r.path.slice(0, cut)}` : root };
}

function view(root: string, rows: ProjectRow[], fields: Partial<ProjectsView> = {}): ProjectsView {
  return {
    seq: 1,
    present: rows.length > 0,
    ready: true,
    answer: "unanswered",
    rows: rows.map((r) => located(root, r)),
    offered: { count: 0, names: [], inner: [] },
    record: { status: "absent" },
    skipped: [],
    ask: null,
    rootCard: card(nameOf(root)),
    rootCategoryStatus: null,
    vocabulary: { file: { status: "absent" }, active: false, tags: [], categories: [] },
    ...fields,
  };
}

const FIXTURES: Record<string, Fixture> = {
  [MOCK_PROJECTS_FRESH]: {
    files: {
      "README.md": "# Workspace\n\nEverything we build.\n",
      "hubs/hub-b/README.md": "# Hub B\n\nThe software.\n",
      "hubs/hub-b/projects/spec/plan.md": "# Spec plan\n",
      "tools/notes-app/README.md": "# Notes app\n",
      "site/index.md": "# Site\n",
    },
    view: () =>
      view(MOCK_PROJECTS_FRESH, [
        row("hubs/hub-b", { marker: "both", inner: 1 }),
        row("hubs/hub-b/projects/spec", { marker: "permark", parent: "hubs/hub-b" }),
        row("site", { marker: "permark" }),
        row("tools/notes-app"),
      ]),
  },
  [MOCK_PROJECTS_LISTED]: {
    files: {
      "README.md": "# Workspace\n",
      "hubs/hub-b/README.md": "# Hub B\n",
      "hubs/hub-b/projects/spec/plan.md": "# Spec plan\n",
      "tools/notes-app/README.md": "# Notes app\n",
      "tools/new-app/README.md": "# New app\n",
      "vendor-lib/README.md": "# Vendor lib\n",
    },
    view: () =>
      view(
        MOCK_PROJECTS_LISTED,
        [
          row("hubs/hub-b", {
            uid: "k7m2q7xd2a",
            state: "included",
            marker: "both",
            name: "PerMark software",
            short: "hub-b",
            description: "PerMark software (core product).",
            category: "software",
            inner: 1,
            cardPlace: "project",
          }),
          row("hubs/hub-b/projects/spec", {
            uid: "spec2spec2",
            state: "included",
            marker: "permark",
            parent: "hubs/hub-b",
            hidden: "own+root",
            cardPlace: "project",
          }),
          row("hubs/hub-x", { uid: "hubx2hubx2", state: "missing", missing: "gone", marker: null }),
          row("tools/new-app"),
          row("tools/notes-app", {
            uid: "notes2note",
            state: "included",
            agent: "off",
            cardPlace: "profile",
          }),
          row("vendor-lib", { state: "declined" }),
        ],
        { answer: "answered", record: { status: "ok" } },
      ),
  },
  [MOCK_PROJECTS_BROKEN]: {
    files: { "README.md": "# Broken\n", "tools/notes-app/README.md": "# Notes app\n" },
    view: () =>
      view(MOCK_PROJECTS_BROKEN, [row("tools/notes-app")], {
        record: { status: "unreadable", line: 3, message: "expected `=`" },
        answer: "answered",
      }),
  },
  [MOCK_PROJECTS_MANY]: {
    files: {
      "README.md": "# Many projects\n",
      ...Object.fromEntries(MANY_NAMES.map((name) => [`repos/${name}/README.md`, `# ${name}\n`])),
      "incoming/new-1/README.md": "# New 1\n",
      "incoming/new-2/README.md": "# New 2\n",
      "incoming/new-3/README.md": "# New 3\n",
    },
    view: () =>
      view(
        MOCK_PROJECTS_MANY,
        [
          ...MANY_NAMES.map((name, n) =>
            row(`repos/${name}`, {
              uid: mockUid(500 + n),
              state: "included",
              marker: n % 6 === 5 ? "permark" : "git",
              cardPlace: "profile",
            }),
          ),
          row("incoming/new-1"),
          row("incoming/new-2", { marker: "permark" }),
          row("incoming/new-3"),
          row("repos/old-gone", {
            uid: "oldgone2ab",
            state: "missing",
            missing: "gone",
            marker: null,
          }),
        ],
        { answer: "answered", record: { status: "ok" } },
      ),
  },
  [MOCK_PROJECTS_ONE]: {
    files: { "README.md": "# Template\n", "template-app/README.md": "# Template app\n" },
    view: () => view(MOCK_PROJECTS_ONE, [row("template-app")]),
  },
  [MOCK_PROJECTS_VOCAB]: {
    files: {
      "overview.md": "---\ntags: [spec, roadmap]\n---\n# Workspace\n",
      "hub-b/plan.md": "---\ntags: [spec, draft]\n---\n# Plan\n",
      "hub-b/notes.md": "---\ntags: [lab]\n---\n# Notes\n",
      "site/index.md": "---\ntags: [draft]\n---\n# Site\n",
    },
    view: () =>
      view(
        MOCK_PROJECTS_VOCAB,
        [
          row("hub-b", {
            uid: "vocabhubb2",
            state: "included",
            marker: "both",
            name: "PerMark software",
            short: "hub-b",
            category: "software",
            cardPlace: "project",
          }),
          row("site", {
            uid: "vocabsite2",
            state: "included",
            marker: "permark",
            category: "website",
            cardPlace: "project",
          }),
          row("tools/scratch", {
            uid: "vocabtool2",
            state: "included",
            category: "experiments",
            cardPlace: "profile",
          }),
        ],
        {
          answer: "answered",
          record: { status: "ok" },
          vocabulary: {
            file: { status: "ok" },
            active: true,
            tags: [
              { name: "needs-review", color: null, icon: null },
              { name: "spec", color: "blue", icon: "book-open" },
            ],
            categories: [{ name: "software", color: "teal", icon: "code" }],
          },
        },
      ),
  },
};

/** What tests and seeds see of the mock projects. */
export type ProjectsProbe = {
  /** Commands asked, in order (`name` + args as JSON). */
  calls: string[];
  /** A project news row in the Changes panel (as the live feed would send it). */
  news(event: ProjectEvent): void;
  /** The view of a folder now (`null`: plain). */
  viewOf(root: string): ProjectsView | null;
  /** Every folder back to its fixture (tests sharing one mock). */
  reset(): void;
  /** Asks "is this the missing one?" in the window's folder (a project moved | renamed while PerMark was closed). */
  askMove(ask: MoveAsk): void;
};

type Deps = {
  /** The folder the window shows. */
  root: () => string;
  /** The tags plugin hears that official lists changed (`tags:changed`). */
  tagsChanged?: () => void;
  emit: <E extends keyof Events>(event: E, payload: Events[E]) => void;
  feed: (item: FeedItem) => void;
  /** The fixture's files join the in-memory project (shown in the Explorer). */
  addFiles: (files: Record<string, string>) => void;
};

type Handlers = Pick<
  Api,
  | "projectsView"
  | "projectsInclude"
  | "projectsDecline"
  | "projectsUndo"
  | "projectsLeaveOut"
  | "projectsSetHidden"
  | "projectsSetAgent"
  | "projectsLocate"
  | "projectsRemove"
  | "projectsAnswerMove"
  | "projectsCardSet"
  | "vocabularyPromote"
  | "vocabularyRename"
  | "vocabularyRemove"
  | "vocabularySetStyle"
>;

export function createMockProjects(deps: Deps): { handlers: Handlers; probe: ProjectsProbe } {
  const views = new Map<string, ProjectsView>();
  const undos = new Map<number, ProjectsView>();
  let feedId = 1_000_000;
  let nextUid = 1;
  const calls: string[] = [];

  /** The view of the window's folder (made from its fixture on first use; plain folders get a plain view). */
  function current(): ProjectsView {
    const root = deps.root();
    const known = views.get(root);
    if (known) return known;
    const fixture = FIXTURES[root];
    const made = marked(root, summarized(fixture ? fixture.view() : view(root, [])));
    if (fixture) deps.addFiles(fixture.files);
    views.set(root, made);
    return made;
  }

  /** Bumps `seq`, tells the window. */
  function commit(next: ProjectsView): void {
    const fixed = marked(deps.root(), summarized({ ...next, seq: next.seq + 1 }));
    views.set(deps.root(), fixed);
    deps.emit("projects-changed", { root: deps.root(), seq: fixed.seq });
  }

  function chosen(v: ProjectsView, pick: ProjectPick, states: ProjectRow["state"][]): Set<string> {
    const wanted = pick.kind === "all" ? null : new Set(pick.paths);
    return new Set(
      v.rows
        .filter((r) => states.includes(r.state) && (wanted === null || wanted.has(r.path)))
        .map((r) => r.path),
    );
  }

  function readOnly(v: ProjectsView): boolean {
    return v.record.status === "unreadable" || v.record.status === "newer";
  }

  function refused<T>(command: string): Promise<Result<T>> {
    return Promise.resolve({
      ok: false,
      error: {
        kind: "backend",
        command: command as never,
        error: { kind: "unavailable", subject: "projects", detail: null },
      },
    });
  }

  function update(change: (r: ProjectRow) => ProjectRow): void {
    const v = current();
    commit({ ...v, rows: v.rows.map(change) });
  }

  /** A typed name by the tag rule (the backend's `normalize_tag`). */
  function vocabName(raw: string): string {
    return raw.trim().toLowerCase().split(/\s+/).join("-");
  }

  /** No `.permark/` in a folder without a project list (the mock's stand-in), or a list that can't be read. */
  function vocabWritable(v: ProjectsView): boolean {
    return (
      v.record.status !== "absent" &&
      v.vocabulary.file.status !== "unreadable" &&
      v.vocabulary.file.status !== "newer"
    );
  }

  function vocabRefused(
    command:
      "vocabulary_promote" | "vocabulary_rename" | "vocabulary_remove" | "vocabulary_set_style",
    kind: "invalidData" | "unavailable" | "notFound" | "exists",
    subject: string,
  ): Promise<Result<null>> {
    return Promise.resolve({
      ok: false,
      error: { kind: "backend", command, error: { kind, subject, detail: null } },
    });
  }

  /** The kind's list replaced (sorted by name like the backend), marks again, windows + tags plugin told. */
  function vocabCommit(v: ProjectsView, kind: VocabularyKind, list: VocabularyEntry[]): void {
    const sorted = [...list].sort((a, b) => a.name.localeCompare(b.name));
    const tags = kind === "tag" ? sorted : v.vocabulary.tags;
    const categories = kind === "category" ? sorted : v.vocabulary.categories;
    commit({
      ...v,
      vocabulary: {
        ...v.vocabulary,
        file: { status: "ok" },
        active: tags.length + categories.length > 0,
        tags,
        categories,
      },
    });
    deps.tagsChanged?.();
  }

  const handlers: Handlers = {
    projectsView: () => {
      calls.push("projects_view");
      return ok(current());
    },
    projectsInclude: (pick) => {
      calls.push(`projects_include ${JSON.stringify(pick)}`);
      const v = current();
      if (readOnly(v)) return refused("projects_include");
      const take = chosen(v, pick, pick.kind === "all" ? ["offered"] : ["offered", "declined"]);
      if (take.size === 0) return ok({ included: 0, undo: null });
      const token = v.seq + 1;
      undos.clear();
      undos.set(token, v);
      commit({
        ...v,
        answer: "answered",
        record: v.record.status === "absent" ? { status: "ok" } : v.record,
        rows: v.rows.map((r) =>
          take.has(r.path)
            ? {
                ...r,
                state: "included",
                uid: r.uid ?? mockUid(nextUid++),
                cardPlace: r.marker === "git" ? "profile" : "project",
              }
            : r,
        ),
      });
      return ok({ included: take.size, undo: token });
    },
    projectsDecline: (pick) => {
      calls.push(`projects_decline ${JSON.stringify(pick)}`);
      const v = current();
      if (pick.kind === "all" && v.answer === "unanswered") {
        commit({
          ...v,
          answer: "declinedAll",
          rows: v.rows.map((r) => (r.state === "offered" ? { ...r, state: "declined" } : r)),
        });
        return ok(null);
      }
      const take = chosen(v, pick, ["offered"]);
      commit({
        ...v,
        rows: v.rows.map((r) => (take.has(r.path) ? { ...r, state: "declined" } : r)),
      });
      return ok(null);
    },
    projectsUndo: (token) => {
      calls.push(`projects_undo ${token}`);
      const before = undos.get(token);
      if (!before) return ok({ undone: false, permarkKept: false });
      undos.clear();
      commit({ ...before, seq: current().seq });
      return ok({ undone: true, permarkKept: false });
    },
    projectsLeaveOut: (uids) => {
      calls.push(`projects_leave_out ${JSON.stringify(uids)}`);
      if (readOnly(current())) return refused("projects_leave_out");
      update((r) =>
        r.uid !== null && uids.includes(r.uid) && r.state === "included"
          ? { ...r, state: "declined", uid: null, cardPlace: null, agent: "same" }
          : r,
      );
      return ok(null);
    },
    projectsSetHidden: (uid, rule) => {
      calls.push(`projects_set_hidden ${uid} ${rule}`);
      if (readOnly(current())) return refused("projects_set_hidden");
      update((r) => (r.uid === uid ? { ...r, hidden: rule } : r));
      return ok(null);
    },
    projectsSetAgent: (uid, choice) => {
      calls.push(`projects_set_agent ${uid} ${choice}`);
      update((r) => (r.uid === uid ? { ...r, agent: choice } : r));
      return ok(null);
    },
    projectsLocate: (uid, path) => {
      calls.push(`projects_locate ${uid} ${path}`);
      const root = deps.root();
      const prefix = `${root}/`;
      if (!path.replace(/\\/g, "/").startsWith(prefix)) return ok("outside");
      const rel = path.replace(/\\/g, "/").slice(prefix.length);
      const v = current();
      if (v.rows.some((r) => r.uid !== null && r.uid !== uid && r.path === rel)) return ok("taken");
      if (!v.rows.some((r) => r.path === rel && r.state !== "missing")) return ok("notProject");
      commit({
        ...v,
        rows: v.rows
          .filter((r) => r.path !== rel)
          .map((r) =>
            r.uid === uid
              ? { ...r, path: rel, state: "included", missing: null, marker: "git" }
              : r,
          ),
      });
      return ok(null);
    },
    projectsRemove: (uids) => {
      calls.push(`projects_remove ${JSON.stringify(uids)}`);
      const v = current();
      commit({
        ...v,
        rows: v.rows.filter(
          (r) => !(r.uid !== null && uids.includes(r.uid) && r.state === "missing"),
        ),
      });
      return ok(null);
    },
    projectsAnswerMove: (uid, path, same) => {
      calls.push(`projects_answer_move ${uid} ${path} ${same}`);
      const v = current();
      if (!same) {
        commit({ ...v, ask: null });
        return ok(null);
      }
      commit({
        ...v,
        ask: null,
        rows: v.rows
          .filter((r) => r.path !== path)
          .map((r) =>
            r.uid === uid ? { ...r, path, state: "included", missing: null, marker: "git" } : r,
          ),
      });
      return ok(null);
    },
    projectsCardSet: (target: CardTarget, change) => {
      calls.push(`projects_card_set ${JSON.stringify(target)} ${JSON.stringify(change)}`);
      const v = current();
      const value = change.value.trim() === "" ? null : change.value.trim();
      const apply = (c: CardDto, fallback: string): CardDto => {
        switch (change.field) {
          case "name":
            return { ...c, name: value ?? fallback };
          case "short":
            return { ...c, short: value ?? fallback };
          case "description":
            return { ...c, description: value };
          case "category":
            return { ...c, category: value?.toLowerCase().replace(/\s+/g, "-") ?? null };
          default:
            return change.field satisfies never;
        }
      };
      if (target.kind === "root") {
        const rootCard = apply(v.rootCard, nameOf(deps.root()));
        commit({ ...v, rootCard });
        return ok(rootCard);
      }
      const member = v.rows.find((r) => r.uid === target.uid && r.state === "included");
      if (!member) {
        return Promise.resolve({
          ok: false,
          error: {
            kind: "backend",
            command: "projects_card_set",
            error: { kind: "notFound", subject: target.uid, detail: null },
          },
        });
      }
      const next = apply(
        {
          name: member.name,
          short: member.short,
          description: member.description,
          category: member.category,
          place: member.cardPlace ?? "profile",
          invalid: [],
          warning: null,
        },
        nameOf(member.path),
      );
      update((r) =>
        r.uid === target.uid
          ? {
              ...r,
              name: next.name,
              short: next.short,
              description: next.description,
              category: next.category,
            }
          : r,
      );
      return ok(next);
    },
    vocabularyPromote: (kind, name) => {
      calls.push(`vocabulary_promote ${kind} ${name}`);
      const v = current();
      const clean = vocabName(name);
      if (clean === "") return vocabRefused("vocabulary_promote", "invalidData", "name");
      if (!vocabWritable(v)) return vocabRefused("vocabulary_promote", "unavailable", "vocabulary");
      const list = kind === "tag" ? v.vocabulary.tags : v.vocabulary.categories;
      if (list.some((e) => e.name === clean)) return ok(null);
      vocabCommit(v, kind, [...list, { name: clean, color: null, icon: null }]);
      return ok(null);
    },
    vocabularyRename: (kind, name, newName) => {
      calls.push(`vocabulary_rename ${kind} ${name} ${newName}`);
      const v = current();
      const [from, to] = [vocabName(name), vocabName(newName)];
      if (from === "") return vocabRefused("vocabulary_rename", "invalidData", "name");
      if (to === "") return vocabRefused("vocabulary_rename", "invalidData", "newName");
      if (!vocabWritable(v)) return vocabRefused("vocabulary_rename", "unavailable", "vocabulary");
      const list = kind === "tag" ? v.vocabulary.tags : v.vocabulary.categories;
      if (!list.some((e) => e.name === from)) {
        return vocabRefused("vocabulary_rename", "notFound", "name");
      }
      if (from === to) return ok(null);
      if (list.some((e) => e.name === to)) {
        return vocabRefused("vocabulary_rename", "exists", "newName");
      }
      vocabCommit(
        v,
        kind,
        list.map((e) => (e.name === from ? { ...e, name: to } : e)),
      );
      return ok(null);
    },
    vocabularyRemove: (kind, name) => {
      calls.push(`vocabulary_remove ${kind} ${name}`);
      const v = current();
      const clean = vocabName(name);
      if (clean === "") return vocabRefused("vocabulary_remove", "invalidData", "name");
      if (!vocabWritable(v)) return vocabRefused("vocabulary_remove", "unavailable", "vocabulary");
      const list = kind === "tag" ? v.vocabulary.tags : v.vocabulary.categories;
      if (!list.some((e) => e.name === clean)) {
        return vocabRefused("vocabulary_remove", "notFound", "name");
      }
      vocabCommit(
        v,
        kind,
        list.filter((e) => e.name !== clean),
      );
      return ok(null);
    },
    vocabularySetStyle: (kind, name, color, icon) => {
      calls.push(`vocabulary_set_style ${kind} ${name} ${color} ${icon}`);
      const v = current();
      const clean = vocabName(name);
      const pickedColor =
        color === null ? null : (COLORS.find((c) => c === color.toLowerCase()) ?? "");
      const pickedIcon = icon === null ? null : (ICONS.find((i) => i === icon.toLowerCase()) ?? "");
      if (clean === "") return vocabRefused("vocabulary_set_style", "invalidData", "name");
      if (pickedColor === "") return vocabRefused("vocabulary_set_style", "invalidData", "color");
      if (pickedIcon === "") return vocabRefused("vocabulary_set_style", "invalidData", "icon");
      if (!vocabWritable(v)) {
        return vocabRefused("vocabulary_set_style", "unavailable", "vocabulary");
      }
      const list = kind === "tag" ? v.vocabulary.tags : v.vocabulary.categories;
      const entry = list.find((e) => e.name === clean);
      if (!entry) return vocabRefused("vocabulary_set_style", "notFound", "name");
      if (entry.color === pickedColor && entry.icon === pickedIcon) return ok(null);
      vocabCommit(
        v,
        kind,
        list.map((e) => (e.name === clean ? { ...e, color: pickedColor, icon: pickedIcon } : e)),
      );
      return ok(null);
    },
  };

  const probe: ProjectsProbe = {
    calls,
    news: (event) => {
      feedId += 1;
      deps.feed({ item: "project", id: feedId, atMs: Date.now(), event });
    },
    viewOf: (root) =>
      views.get(root) ?? (FIXTURES[root] ? marked(root, summarized(FIXTURES[root].view())) : null),
    reset: () => {
      views.clear();
      undos.clear();
      calls.length = 0;
    },
    askMove: (ask) => commit({ ...current(), ask }),
  };
  return { handlers, probe };
}
