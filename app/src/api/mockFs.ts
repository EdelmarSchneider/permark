// A small in-memory project for the mock backend (dev:mock, tests): files, folders, a simplified filter, moves with a
// link preview, delete, new, undo. Its files are the ones the editor and the document view read and write (one store,
// as one disk). It mirrors the shapes and the refusals of the real backend (names, protected folders, case, unsaved
// documents), not its every rule (the Rust side has those, with their tests).

import { diffLists, nameOf, parentOf, renamedPath } from "../core/explorer/list";
import { IDENTITY_UNKNOWN } from "./error";
import { MOCK_EDIT_SOURCES, mockTextOf } from "./mockDocs";
import { checkName, newFileName } from "./mockNames";
import type {
  Category,
  MoveOutcome,
  MovePlan,
  MoveSpec,
  NewKind,
  PlanFile,
  TreeFilter,
  TreeNode,
} from "./types";

const EXTENSIONS: Record<string, Category> = {
  md: "markdown",
  markdown: "markdown",
  txt: "text",
  toml: "config",
  yml: "config",
  yaml: "config",
  json: "data",
  csv: "data",
  png: "image",
  jpg: "image",
  svg: "image",
  mp3: "audio",
  mp4: "video",
  docx: "office",
  pdf: "office",
  rs: "code",
  ts: "code",
  js: "code",
  py: "code",
  sh: "code",
  css: "code",
  html: "code",
};

export function categoryOfName(name: string): Category {
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return "other";
  return EXTENSIONS[name.slice(dot + 1).toLowerCase()] ?? "other";
}

const PRESETS: Record<string, readonly Category[]> = {
  documents: ["markdown", "text"],
  "docs-and-assets": ["markdown", "text", "config", "data", "image", "audio", "video"],
  everything: [
    "markdown",
    "text",
    "config",
    "data",
    "image",
    "audio",
    "video",
    "office",
    "code",
    "other",
  ],
};

/** A document saved on Windows: UTF-8 with a BOM, CRLF line breaks (the editor's encoding and line ending paths). */
export const MOCK_CRLF_BOM_DOC = "docs/windows.md";

/** The mock project's files. A file the fixture views were rendered from has that text: what the view shows, the
 *  editor edits and the project moves is one file. */
export function seedFiles(): Record<string, string> {
  // the fixture documents (README.md, docs/guide.md, ...) come with their texts below, and so do the files the mock
  // shows as text (`mockTextOf`)
  const files: Record<string, string> = {
    "docs/img/logo.png": "png",
    "notes/todo.txt": "",
    "package.json": "",
    ".github/ci.yml": "",
    "src/main.rs": "",
    [MOCK_CRLF_BOM_DOC]:
      "\uFEFF# Saved on Windows\r\n\r\nLines end with CR LF.\r\nThe file starts with a BOM.\r\n",
  };
  for (const [path, source] of MOCK_EDIT_SOURCES) files[path] = source.text;
  for (const path of Object.keys(files)) files[path] = mockTextOf(path) ?? files[path] ?? "";
  return files;
}

/** The spelling `path` has in `files`, matched without case as on Windows and macOS (`paths::fold`); `null`: none. */
export function storedPath(files: ReadonlyMap<string, string>, path: string): string | null {
  if (files.has(path)) return path;
  const key = path.toLowerCase();
  for (const stored of files.keys()) if (stored.toLowerCase() === key) return stored;
  return null;
}

/** `path` is `folder` or inside it (folded). */
function within(path: string, folder: string): boolean {
  const a = path.toLowerCase();
  const b = folder.toLowerCase();
  return a === b || a.startsWith(`${b}/`);
}

/** The project folder itself (or a worktree's): never moved or deleted. */
function isRoot(path: string): boolean {
  return path === "" || /^@wt\/[^/]+$/.test(path);
}

/** Folders the app keeps for itself or that make a folder a repository (`fileops/ops.rs` `is_protected`). */
function isProtected(path: string): boolean {
  const rel = path.startsWith("@wt/") ? path.split("/").slice(2).join("/") : path;
  const parts = rel.split("/").map((p) => p.toLowerCase());
  return parts[0] === ".permark" || parts.includes(".git");
}

/** A document whose links a move rewrites: what the preview saw and the changes it showed. */
type PlannedDoc = { path: string; seen: string; changes: PlanFile["changes"] };

type Op =
  | { kind: "move"; moves: MoveSpec[]; before: Map<string, string> }
  | { kind: "new"; path: string }
  | { kind: "delete"; paths: string[]; files: Map<string, string> }
  | { kind: "import"; paths: string[] };

export class MockProject {
  readonly files = new Map<string, string>();
  readonly dirs = new Set<string>();
  /** Folders made in the Explorer: shown while empty. */
  readonly kept = new Set<string>();
  /** Names of the linked worktrees of the vault, in its order (their files sit below `@wt/<name>/`). */
  readonly worktrees: string[] = [];
  /** The drive gives no file ids (FAT, some network drives): a rename that only changes case cannot be proven to keep
   *  one file (`fsx::same_file` answers Unknown), so it is refused. */
  fileIdsUnknown = false;
  readonly #undo: (Op & { id: number })[] = [];
  #nextUndo = 1;
  readonly #plans = new Map<number, { moves: MoveSpec[]; docs: PlannedDoc[] }>();
  #nextPlan = 1;

  constructor(seed: Record<string, string> = seedFiles()) {
    for (const [path, text] of Object.entries(seed)) this.add(path, text);
  }

  add(path: string, text: string): void {
    this.files.set(path, text);
    for (let at = parentOf(path); at !== ""; at = parentOf(at)) this.dirs.add(at);
  }

  /** Something is at `path`, in any case (the mock models a Windows volume). */
  exists(path: string): boolean {
    return this.#stored(path) !== null;
  }

  /** The spelling of the file or folder at `path` (folded), or `null`. */
  #stored(path: string): string | null {
    if (this.files.has(path) || this.dirs.has(path)) return path;
    const file = storedPath(this.files, path);
    if (file !== null) return file;
    const key = path.toLowerCase();
    for (const dir of this.dirs) if (dir.toLowerCase() === key) return dir;
    return null;
  }

  #isDir(path: string): boolean {
    const stored = this.#stored(path);
    return stored !== null && this.dirs.has(stored);
  }

  /** The flat list a filter shows: folders first, names case-insensitive, parents before children. */
  list(filter: TreeFilter): TreeNode[] {
    const categories = new Set(
      filter.show === "custom" ? filter.categories : (PRESETS[filter.show] ?? []),
    );
    const out: TreeNode[] = [];
    const walk = (folder: string): boolean => {
      const names = new Set<string>();
      for (const d of this.dirs) if (parentOf(d) === folder) names.add(d);
      const folders = [...names].sort((a, b) =>
        nameOf(a).toLowerCase().localeCompare(nameOf(b).toLowerCase()),
      );
      const files = [...this.files.keys()]
        .filter((p) => parentOf(p) === folder)
        .sort((a, b) => nameOf(a).toLowerCase().localeCompare(nameOf(b).toLowerCase()));
      let any = false;
      for (const dir of folders) {
        const hidden = nameOf(dir).startsWith(".");
        if (hidden && !filter.showHidden) continue;
        const mark = out.length;
        out.push({
          path: dir,
          name: nameOf(dir),
          kind: { type: "dir", unlisted: null },
          hidden,
          ignored: false,
          forced: false,
        });
        const inside = walk(dir);
        if (!inside && filter.hideEmptyDirs && !this.kept.has(dir)) out.length = mark;
        else any = true;
      }
      for (const path of files) {
        const name = nameOf(path);
        const hidden = name.startsWith(".");
        const category = categoryOfName(name);
        if ((hidden && !filter.showHidden) || !categories.has(category)) continue;
        out.push({
          path,
          name,
          kind: { type: "file", category },
          hidden,
          ignored: false,
          forced: false,
        });
        any = true;
      }
      return any;
    };
    walk("");
    return out;
  }

  counts(filter: TreeFilter): { total: number; shown: number; codeHidden: number } {
    const list = this.list(filter);
    const shown = list.filter((n) => n.kind.type === "file").length;
    const visible = [...this.files.keys()].filter(
      (p) => filter.showHidden || !p.split("/").some((s) => s.startsWith(".")),
    );
    const listed = new Set(list.map((n) => n.path));
    const codeHidden = visible.filter(
      (p) => categoryOfName(nameOf(p)) === "code" && !listed.has(p),
    ).length;
    return { total: visible.length, shown, codeHidden };
  }

  // ---- operations ----------------------------------------------------------------------------------------------

  /** The checks of `fileops/ops.rs` `check_moves`, in its order. */
  #checkMoves(moves: readonly MoveSpec[]): void {
    if (moves.length === 0) throw new MockRefusal("unavailable", null);
    for (const m of moves) {
      for (const path of [m.from, m.to]) {
        if (isRoot(path) || isProtected(path)) throw new MockRefusal("protected", path);
      }
      const problem = checkName(nameOf(m.to));
      if (problem !== null) throw new MockRefusal("invalidName", null, problem);
      if (!this.exists(m.from)) throw new MockRefusal("notFound", m.from);
      // the same entry under another case is a rename; the same name is nothing to do
      const sameKey = m.from.toLowerCase() === m.to.toLowerCase();
      const caseOnly = sameKey && m.from !== m.to;
      if (sameKey && !caseOnly) throw new MockRefusal("unavailable", null);
      if (this.#isDir(m.from) && within(m.to, m.from) && !caseOnly) {
        throw new MockRefusal("intoItself", m.from);
      }
      const parent = parentOf(m.to);
      if (parent !== "" && !this.#isDir(parent)) throw new MockRefusal("notFound", parent);
      if (!caseOnly && this.exists(m.to)) throw new MockRefusal("exists", m.to);
      if (caseOnly && this.fileIdsUnknown) throw new MockRefusal("exists", m.to, IDENTITY_UNKNOWN);
    }
    moves.forEach((a, at) => {
      for (const b of moves.slice(at + 1)) {
        if (within(a.from, b.from) || within(b.from, a.from)) {
          throw new MockRefusal("overlap", a.from);
        }
        if (within(a.to, b.to) || within(b.to, a.to)) throw new MockRefusal("overlap", a.to);
      }
    });
  }

  /** Checks the moves and finds the links to change; a document with unsaved edits (`unsaved`) is left to its
   *  editor (skipped `unsaved`), as `plan_move_with`. Nothing is touched. */
  plan(moves: MoveSpec[], updateLinks: boolean, unsaved: readonly string[] = []): MovePlan {
    this.#checkMoves(moves);
    const files: PlanFile[] = [];
    const docs: PlannedDoc[] = [];
    const skipped: MovePlan["skipped"] = [];
    const held = new Set(unsaved.map((p) => p.toLowerCase()));
    if (updateLinks) {
      for (const [path, text] of this.files) {
        if (!path.endsWith(".md")) continue;
        const changes = text.split("\n").flatMap((line, at) =>
          moves
            .filter((m) => linksTo(line, relativeLink(path, m.from)))
            .map((m) => ({
              line: at + 1,
              old: relativeLink(path, m.from),
              new: relativeLink(path, m.to),
            })),
        );
        if (changes.length === 0) continue;
        if (held.has(path.toLowerCase())) {
          skipped.push({ path, reason: "unsaved" });
          continue;
        }
        const movedTo = renamedPath(path, moves);
        files.push({ path, movedTo: movedTo === path ? null : movedTo, changes });
        docs.push({ path, seen: text, changes });
      }
    }
    const planId = this.#nextPlan++;
    this.#plans.set(planId, { moves, docs });
    return {
      planId,
      moves,
      linkCount: files.reduce((n, f) => n + f.changes.length, 0),
      fileCount: files.length,
      files,
      truncated: false,
      skipped,
      unchecked: [],
    };
  }

  /** The moves of a kept plan (for the rename hints). */
  moves(planId: number): MoveSpec[] {
    return this.#plans.get(planId)?.moves ?? [];
  }

  /** Makes the moves of a preview (checked again), then rewrites the links of the documents it showed, each only if
   *  it still is what the preview saw. */
  apply(planId: number, updateLinks: boolean): MoveOutcome {
    const plan = this.#plans.get(planId);
    if (!plan) throw new MockRefusal("unavailable", null);
    this.#plans.delete(planId);
    const { moves } = plan;
    this.#checkMoves(moves);
    const before = new Map(this.files);
    let links = 0;
    let documents = 0;
    const skipped: MoveOutcome["skipped"] = [];
    if (updateLinks) {
      for (const doc of plan.docs) {
        const now = this.files.get(doc.path);
        const at = renamedPath(doc.path, moves);
        if (now === undefined) skipped.push({ path: at, reason: "missing" });
        else if (now !== doc.seen) skipped.push({ path: at, reason: "changed" });
        else {
          let next = now;
          for (const m of moves) {
            const [from, to] = [relativeLink(doc.path, m.from), relativeLink(doc.path, m.to)];
            next = next.split(`(${from})`).join(`(${to})`).split(`(${from}#`).join(`(${to}#`);
          }
          this.files.set(doc.path, next);
          links += doc.changes.length;
          documents += 1;
        }
      }
    }
    for (const m of moves) this.#rename(this.#stored(m.from) ?? m.from, m.to);
    this.#push({ kind: "move", moves, before });
    return { moved: moves.length, links, documents, skipped };
  }

  #rename(from: string, to: string): void {
    for (const [path, text] of [...this.files]) {
      if (path === from || path.startsWith(`${from}/`)) {
        this.files.delete(path);
        this.add(to + path.slice(from.length), text);
      }
    }
    for (const dir of [...this.dirs]) {
      if (dir === from || dir.startsWith(`${from}/`)) {
        this.dirs.delete(dir);
        this.dirs.add(to + dir.slice(from.length));
      }
    }
  }

  /** Deletes every path or none (all checked first, as `check_deletes`); a path inside another one goes with it.
   *  Returns what went. */
  delete(paths: string[]): string[] {
    if (paths.length === 0) throw new MockRefusal("unavailable", null);
    const checked: string[] = [];
    for (const path of paths) {
      if (isRoot(path) || isProtected(path)) throw new MockRefusal("protected", path);
      const stored = this.#stored(path);
      if (stored === null) throw new MockRefusal("notFound", path);
      checked.push(stored);
    }
    const kept = checked.filter(
      (path, at) =>
        !checked.some((other, i) => i !== at && within(path, other) && (path !== other || i < at)),
    );
    const files = new Map<string, string>();
    for (const path of kept) {
      for (const [p, text] of [...this.files]) {
        if (p === path || p.startsWith(`${path}/`)) {
          files.set(p, text);
          this.files.delete(p);
        }
      }
      for (const d of [...this.dirs])
        if (d === path || d.startsWith(`${path}/`)) this.dirs.delete(d);
    }
    this.#push({ kind: "delete", paths: kept, files });
    return kept;
  }

  /** Items an import put in place (project paths): one undo step takes them out again. Returns its journal id. */
  recordImport(paths: string[]): number {
    return this.#push({ kind: "import", paths });
  }

  #push(op: Op): number {
    const id = this.#nextUndo++;
    this.#undo.push({ ...op, id });
    return id;
  }

  /** Paths an operation touched (a newer one on them makes an older one unsafe to take back out of order). */
  static #paths(op: Op): string[] {
    switch (op.kind) {
      case "move":
        return op.moves.flatMap((m) => [m.from, m.to]);
      case "new":
        return [op.path];
      case "delete":
      case "import":
        return op.paths;
      default:
        return op satisfies never;
    }
  }

  /** Takes back exactly the operation `id` unless gone or touched by a newer one; whether it did. */
  undoEntry(id: number): boolean {
    const at = this.#undo.findIndex((op) => op.id === id);
    const op = this.#undo[at];
    if (op === undefined) return false;
    const mine = MockProject.#paths(op);
    const near = (a: string, b: string): boolean =>
      a === b || a.startsWith(`${b}/`) || b.startsWith(`${a}/`);
    const touched = this.#undo
      .slice(at + 1)
      .some((later) => MockProject.#paths(later).some((p) => mine.some((m) => near(p, m))));
    if (touched) return false;
    this.#undo.splice(at, 1);
    this.#takeBack(op);
    return true;
  }

  /** A new empty file (`.md` | `.txt`) or folder in `dir`, by the backend's rules (`FileOps::create`). */
  create(dir: string, name: string, kind: NewKind): string {
    let full = name;
    if (kind === "file") {
      const named = newFileName(name);
      if (!named.ok) throw new MockRefusal("invalidName", null, named.problem);
      full = named.name;
    } else {
      const problem = checkName(name);
      if (problem !== null) throw new MockRefusal("invalidName", null, problem);
    }
    if (isProtected(dir)) throw new MockRefusal("protected", dir);
    if (dir !== "" && !this.#isDir(dir)) throw new MockRefusal("notFound", dir);
    const path = dir === "" ? full : `${dir}/${full}`;
    if (this.exists(path)) throw new MockRefusal("exists", path);
    if (kind === "file") this.add(path, "");
    else {
      this.dirs.add(path);
      this.kept.add(path);
    }
    this.#push({ kind: "new", path });
    return path;
  }

  /** Takes the newest operation back; false when there is none. */
  undo(): boolean {
    const op = this.#undo.pop();
    if (!op) return false;
    this.#takeBack(op);
    return true;
  }

  #takeBack(op: Op): void {
    switch (op.kind) {
      case "move":
        for (const m of [...op.moves].reverse()) this.#rename(m.to, m.from);
        for (const [path, text] of op.before) if (this.files.has(path)) this.files.set(path, text);
        return;
      case "new":
        this.files.delete(op.path);
        this.dirs.delete(op.path);
        return;
      case "delete":
        for (const [path, text] of op.files) this.add(path, text);
        return;
      case "import":
        for (const path of op.paths) {
          for (const p of [...this.files.keys()]) {
            if (p === path || p.startsWith(`${path}/`)) this.files.delete(p);
          }
          for (const d of [...this.dirs])
            if (d === path || d.startsWith(`${path}/`)) this.dirs.delete(d);
        }
        return;
      default:
        op satisfies never;
    }
  }

  diff(before: readonly TreeNode[], after: readonly TreeNode[]): ReturnType<typeof diffLists> {
    return diffLists(before, after);
  }
}

/** What the mock refuses, with the kind, subject and detail the real backend would answer. */
export class MockRefusal extends Error {
  constructor(
    readonly kind:
      | "notFound"
      | "exists"
      | "intoItself"
      | "invalidName"
      | "unavailable"
      | "protected"
      | "overlap",
    readonly subject: string | null,
    readonly detail: string | null = null,
  ) {
    super(kind);
  }
}

/** `line` holds a Markdown link to `href`, with or without an anchor. */
function linksTo(line: string, href: string): boolean {
  return line.includes(`(${href})`) || line.includes(`(${href}#`);
}

/** The way from the document `doc` to `target` (both project paths). */
function relativeLink(doc: string, target: string): string {
  const from = parentOf(doc)
    .split("/")
    .filter((s) => s !== "");
  const to = target.split("/");
  let common = 0;
  while (common < from.length && common < to.length - 1 && from[common] === to[common]) common += 1;
  return [...Array<string>(from.length - common).fill(".."), ...to.slice(common)].join("/");
}
