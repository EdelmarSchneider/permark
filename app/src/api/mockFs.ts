// A small in-memory project for the mock backend (dev:mock, tests): files, folders, a simplified filter, moves with a
// link preview, delete, new, undo. It mirrors the shapes of the real backend, not its every rule (the Rust side has
// those, with their tests).

import { diffLists, nameOf, parentOf } from "../core/explorer/list";
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
};

export function categoryOfName(name: string): Category {
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return "other";
  return EXTENSIONS[name.slice(dot + 1).toLowerCase()] ?? "other";
}

const PRESETS: Record<string, readonly Category[]> = {
  documents: ["markdown", "text"],
  "docs-and-assets": ["markdown", "text", "config", "data", "image", "audio", "video"],
  everything: ["markdown", "text", "config", "data", "image", "audio", "video", "office", "other"],
};

export function seedFiles(): Record<string, string> {
  return {
    "CLAUDE.md": "# Rules\n",
    "README.md": "# Demo\n\nSee [the guide](docs/guide.md) and [notes](notes/ideas.md).\n",
    "docs/guide.md":
      "# Guide\n\nBack to the [readme](../README.md). Ideas: [i](../notes/ideas.md).\n",
    "docs/img/logo.png": "png",
    "notes/ideas.md": "# Ideas\n\n- one\n",
    "notes/todo.txt": "todo",
    "package.json": "{}",
    ".github/ci.yml": "name: ci",
    "src/main.rs": "fn main() {}",
  };
}

type Op =
  | { kind: "move"; moves: MoveSpec[]; before: Map<string, string> }
  | { kind: "new"; path: string }
  | { kind: "delete"; paths: string[]; files: Map<string, string> };

export class MockProject {
  readonly files = new Map<string, string>();
  readonly dirs = new Set<string>();
  /** Folders made in the Explorer: shown while empty. */
  readonly kept = new Set<string>();
  readonly #undo: Op[] = [];
  readonly #plans = new Map<number, MoveSpec[]>();
  #nextPlan = 1;

  constructor(seed: Record<string, string> = seedFiles()) {
    for (const [path, text] of Object.entries(seed)) this.add(path, text);
  }

  add(path: string, text: string): void {
    this.files.set(path, text);
    for (let at = parentOf(path); at !== ""; at = parentOf(at)) this.dirs.add(at);
  }

  exists(path: string): boolean {
    return this.files.has(path) || this.dirs.has(path);
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

  counts(filter: TreeFilter): { total: number; shown: number } {
    const shown = this.list(filter).filter((n) => n.kind.type === "file").length;
    const total = [...this.files.keys()].filter(
      (p) => filter.showHidden || !p.split("/").some((s) => s.startsWith(".")),
    ).length;
    return { total, shown };
  }

  // ---- operations ----------------------------------------------------------------------------------------------

  plan(moves: MoveSpec[], updateLinks: boolean): MovePlan {
    for (const m of moves) {
      if (!this.exists(m.from)) throw new MockRefusal("notFound", m.from);
      if (this.exists(m.to)) throw new MockRefusal("exists", m.to);
      if (m.to === m.from || m.to.startsWith(`${m.from}/`))
        throw new MockRefusal("intoItself", m.from);
      if (!this.dirs.has(parentOf(m.to)) && parentOf(m.to) !== "")
        throw new MockRefusal("notFound", parentOf(m.to));
    }
    const files: PlanFile[] = [];
    if (updateLinks) {
      for (const [path, text] of this.files) {
        if (!path.endsWith(".md")) continue;
        const changes = text.split("\n").flatMap((line, at) =>
          moves
            .filter((m) => line.includes(`(${relativeLink(path, m.from)})`))
            .map((m) => ({
              line: at + 1,
              old: relativeLink(path, m.from),
              new: relativeLink(path, m.to),
            })),
        );
        if (changes.length > 0) files.push({ path, movedTo: null, changes });
      }
    }
    const planId = this.#nextPlan++;
    this.#plans.set(planId, moves);
    return {
      planId,
      moves,
      linkCount: files.reduce((n, f) => n + f.changes.length, 0),
      fileCount: files.length,
      files,
      truncated: false,
      skipped: [],
    };
  }

  /** The moves of a kept plan (for the rename hints). */
  moves(planId: number): MoveSpec[] {
    return this.#plans.get(planId) ?? [];
  }

  apply(planId: number, updateLinks: boolean): MoveOutcome {
    const moves = this.#plans.get(planId);
    if (!moves) throw new MockRefusal("unavailable", "");
    this.#plans.delete(planId);
    const before = new Map(this.files);
    let links = 0;
    const touched = new Set<string>();
    if (updateLinks) {
      for (const [path, text] of this.files) {
        let next = text;
        for (const m of moves) {
          const old = `(${relativeLink(path, m.from)})`;
          if (next.includes(old)) {
            next = next.split(old).join(`(${relativeLink(path, m.to)})`);
            links += 1;
            touched.add(path);
          }
        }
        this.files.set(path, next);
      }
    }
    for (const m of moves) this.#rename(m.from, m.to);
    this.#undo.push({ kind: "move", moves, before });
    return { moved: moves.length, links, documents: touched.size, skipped: [] };
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

  delete(paths: string[]): void {
    const files = new Map<string, string>();
    for (const path of paths) {
      if (!this.exists(path)) throw new MockRefusal("notFound", path);
      for (const [p, text] of [...this.files]) {
        if (p === path || p.startsWith(`${path}/`)) {
          files.set(p, text);
          this.files.delete(p);
        }
      }
      for (const d of [...this.dirs])
        if (d === path || d.startsWith(`${path}/`)) this.dirs.delete(d);
    }
    this.#undo.push({ kind: "delete", paths, files });
  }

  create(dir: string, name: string, kind: NewKind): string {
    const full = kind === "file" && !/\.[a-z]+$/i.test(name) ? `${name}.md` : name;
    if (/[\\/:*?"<>|]/.test(full)) throw new MockRefusal("invalidName", "chars");
    const path = dir === "" ? full : `${dir}/${full}`;
    if (this.exists(path)) throw new MockRefusal("exists", path);
    if (kind === "file") this.add(path, "");
    else {
      this.dirs.add(path);
      this.kept.add(path);
    }
    this.#undo.push({ kind: "new", path });
    return path;
  }

  /** Takes the newest operation back; false when there is none. */
  undo(): boolean {
    const op = this.#undo.pop();
    if (!op) return false;
    switch (op.kind) {
      case "move":
        for (const m of [...op.moves].reverse()) this.#rename(m.to, m.from);
        for (const [path, text] of op.before) if (this.files.has(path)) this.files.set(path, text);
        return true;
      case "new":
        this.files.delete(op.path);
        this.dirs.delete(op.path);
        return true;
      case "delete":
        for (const [path, text] of op.files) this.add(path, text);
        return true;
      default:
        return op satisfies never;
    }
  }

  diff(before: readonly TreeNode[], after: readonly TreeNode[]): ReturnType<typeof diffLists> {
    return diffLists(before, after);
  }
}

/** What the mock refuses, with the kind the real backend would answer. */
export class MockRefusal extends Error {
  constructor(
    readonly kind: "notFound" | "exists" | "intoItself" | "invalidName" | "unavailable",
    readonly subject: string,
  ) {
    super(kind);
  }
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
