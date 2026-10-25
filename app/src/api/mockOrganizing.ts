// What the organizing plugins' backend halves answer in the mock backend (`dev:mock`, tests): tags and descriptions,
// comments, silence marks and the broken links of the in-memory project. The same shapes the real plugins serve; the
// rules (anchors, files) are the Rust side's and have their tests there.

import { isQuiet } from "../core/silence/silence.svelte";
import { COLORS, ICONS, normalizeDescription, normalizeTags } from "../plugins/tags/model";
import type { MockProject } from "./mockFs";
import { mockTagStatus } from "./mockProjects";
import type {
  CommentView,
  Interest,
  MetaProblem,
  PluginEventName,
  PluginEvents,
  PluginMethod,
  PluginMethods,
  ProjectsView,
  SearchOptions,
} from "./types";

type Handlers = Pick<
  { [M in PluginMethod]: (args: PluginMethods[M]["args"]) => PluginMethods[M]["result"] },
  | "tags.index"
  | "tags.overview"
  | "tags.status"
  | "tags.filesWith"
  | "tags.fileMeta"
  | "tags.setTags"
  | "tags.setDescription"
  | "tags.setStyle"
  | "tags.orphans"
  | "tags.cleanUp"
  | "comments.list"
  | "comments.add"
  | "comments.setDone"
  | "comments.remove"
  | "comments.reply"
  | "quiet.rules"
  | "quiet.set"
  | "broken-links.list"
>;

export class MockPluginError extends Error {
  constructor(
    readonly kind:
      | "unavailable"
      | "notFound"
      | "invalidData"
      | "denied"
      | "exists"
      | "invalidName"
      | "intoItself"
      | "conflict",
    readonly subject: string | null = null,
  ) {
    super(kind);
  }
}

export type OrganizingProbe = {
  /** Makes `meta.toml` unreadable (a merge conflict at that line), or readable again (`null`). */
  tagsProblem(problem: MetaProblem | null): void;
  /** The tags the app keeps for a file now. */
  appTags(path: string): string[];
  /** Comments as they are kept (all of them). */
  comments(): CommentView[];
  /** The silence marks now. */
  marks(): Record<string, Interest>;
  /** Forgets everything the plugins hold (tags, looks, comments, marks, a broken file). */
  reset(): void;
};

const MARKDOWN = /\.(md|markdown|mdown|mkd|mdx|mdc)$/i;
const fold = (text: string): string => text.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");

/** The `tags:` of a document's front matter (a list in brackets or a comma list). */
function frontMatterTags(text: string): string[] {
  const block = /^---\r?\n([\s\S]*?)\r?\n---/.exec(text);
  const line = block?.[1]?.split(/\r?\n/).find((l) => l.startsWith("tags:"));
  if (line === undefined) return [];
  const value = line.slice(5).trim().replace(/^\[/, "").replace(/\]$/, "");
  return normalizeTags(value.split(",").map((s) => s.trim().replace(/^["']|["']$/g, "")));
}

function parentOf(path: string): string {
  const at = path.lastIndexOf("/");
  return at < 0 ? "" : path.slice(0, at);
}

/** The project path a link written in `doc` names (no disk): relative to the document, `..` resolved, `%20` decoded. */
function targetOf(doc: string, href: string): string | null {
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("#")) return null;
  const clean = decodeURIComponent((href.split("#")[0] ?? "").split("?")[0] ?? "").replace(
    /\\/g,
    "/",
  );
  if (clean === "") return null;
  const parts = clean.startsWith("/")
    ? []
    : parentOf(doc)
        .split("/")
        .filter((p) => p !== "");
  for (const part of clean.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (parts.length === 0) return "\0outside";
      parts.pop();
    } else parts.push(part);
  }
  return parts.join("/");
}

export function createMockOrganizing(
  emit: <E extends PluginEventName>(event: E, data: PluginEvents[E]) => void,
  project: MockProject,
  /** The projects view of the window's folder now (official lists, included projects; round 4 status marks). */
  projects: () => { view: ProjectsView | null; root: string } = () => ({ view: null, root: "" }),
): {
  handlers: Handlers;
  allows(options: SearchOptions): (path: string) => boolean;
  probe: OrganizingProbe;
} {
  // ---- tags ----
  const app = new Map<string, { tags: string[]; description: string }>();
  const styles = new Map<string, { color: string | null; icon: string | null }>();
  let problem: MetaProblem | null = null;
  let rev = 1;
  const changed = (): void => {
    rev += 1;
    emit("tags:changed", null);
  };
  const documents = (): string[] => [...project.files.keys()].filter((p) => MARKDOWN.test(p));
  const fileTags = (path: string): string[] =>
    MARKDOWN.test(path) ? frontMatterTags(project.files.get(path) ?? "") : [];
  const tagsOf = (path: string): string[] => [
    ...fileTags(path),
    ...(app.get(path)?.tags ?? []).filter((t) => !fileTags(path).includes(t)),
  ];
  const universe = (): string[] => [...new Set([...documents(), ...app.keys()])].sort();
  const status = (): PluginMethods["tags.status"]["result"] => ({
    location: "project",
    problem,
    writeError: null,
    parked: 0,
  });
  const mark = (name: string, path: string | null): ReturnType<typeof mockTagStatus> => {
    const { view, root } = projects();
    return mockTagStatus(view, root, "tag", name, path);
  };
  const meta = (path: string): PluginMethods["tags.fileMeta"]["result"] => {
    const mine = app.get(path)?.tags ?? [];
    const names = tagsOf(path);
    return {
      path,
      tags: names.map((name) => ({
        name,
        file: fileTags(path).includes(name),
        app: mine.includes(name),
        ...(path.startsWith("@wt/") ? { status: null, owner: null } : mark(name, path)),
      })),
      description: app.get(path)?.description ?? "",
      editable: problem === null && !path.startsWith("@wt/"),
    };
  };
  const refuse = (): void => {
    if (problem !== null) throw new MockPluginError("unavailable", "meta.toml");
  };
  const put = (path: string, next: { tags: string[]; description: string }): void => {
    if (next.tags.length === 0 && next.description === "") app.delete(path);
    else app.set(path, next);
    changed();
  };

  // ---- comments ----
  type Stored = {
    id: string;
    path: string;
    text: string;
    quote: string;
    headingPath: string[];
    line: number;
    endLine: number;
    createdMs: number;
    doneMs: number | null;
    replies: { atMs: number; by: "user" | "agent"; text: string }[];
  };
  const stored: Stored[] = [];
  let next = 1;
  const view = (c: Stored): CommentView => {
    const text = project.files.get(c.path);
    const lines = text?.split("\n") ?? [];
    const at = lines.findIndex((l) => fold(l).includes(fold(c.quote)));
    const state =
      text === undefined
        ? "documentMissing"
        : at >= 0
          ? at + 1 === c.line
            ? "anchored"
            : "moved"
          : "passageChanged";
    return {
      id: c.id,
      path: c.path,
      text: c.text,
      quote: c.quote,
      headingPath: c.headingPath,
      line: at >= 0 ? at + 1 : c.line,
      endLine: at >= 0 ? at + 1 : c.endLine,
      createdMs: c.createdMs,
      state,
      done: c.doneMs !== null,
      doneMs: c.doneMs,
      doneBy: c.doneMs === null ? null : "user",
      reply: null,
      replies: c.replies.map((r) => ({ ...r })),
    };
  };

  // ---- silence ----
  let marks: Record<string, Interest> = {};

  // ---- broken links ----
  const broken = (): PluginMethods["broken-links.list"]["result"] => {
    const docs: PluginMethods["broken-links.list"]["result"]["docs"] = [];
    let total = 0;
    // the opened folder first, then each linked worktree (`@wt/<name>`), as the backend lists and counts them
    const rootOf = (path: string): string =>
      path.startsWith("@wt/") ? path.split("/").slice(0, 2).join("/") : "";
    const order = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0);
    const paths = documents().sort((a, b) => {
      const [ra, rb] = [rootOf(a), rootOf(b)];
      if (ra === rb) return order(a, b);
      return ra === "" ? -1 : rb === "" ? 1 : order(ra, rb);
    });
    const roots = new Map<string, number>([["", 0]]);
    for (const path of paths) {
      const root = rootOf(path);
      if (!roots.has(root)) roots.set(root, 0);
      const links: { line: number; written: string }[] = [];
      (project.files.get(path) ?? "").split("\n").forEach((line, at) => {
        for (const m of line.matchAll(/\]\(([^)\s]+)\)/g)) {
          const href = m[1] ?? "";
          const target = targetOf(path, href);
          if (target === null) continue;
          const exists = project.files.has(target) || project.dirs.has(target.replace(/\/$/, ""));
          if (!exists && !project.files.has(`${target}.md`))
            links.push({ line: at + 1, written: href });
        }
      });
      if (links.length > 0) docs.push({ path, links });
      total += links.length;
      roots.set(root, (roots.get(root) ?? 0) + links.length);
    }
    return {
      ready: true,
      docs,
      total,
      current: roots.get("") ?? 0,
      roots: [...roots].map(([prefix, n]) => ({ prefix, total: n })),
      truncated: false,
      skipped: 0,
    };
  };

  const handlers: Handlers = {
    "tags.index": () => ({
      rev,
      entries: universe()
        .map((path) => ({
          path,
          tags: tagsOf(path),
          description: app.get(path)?.description ?? "",
        }))
        .filter((e) => e.tags.length > 0 || e.description !== ""),
      styles: Object.fromEntries([
        ...[...styles].map(([tag, s]) => [tag, { ...s }]),
        // an official look wins over the app's own
        ...(projects().view?.vocabulary.tags ?? [])
          .filter((e) => e.color !== null || e.icon !== null)
          .map((e) => [e.name, { color: e.color, icon: e.icon }]),
      ]),
      status: status(),
    }),
    "tags.overview": () => {
      const found = new Map<string, { count: number; onlyInFiles: boolean }>();
      for (const path of universe()) {
        for (const name of tagsOf(path)) {
          const slot = found.get(name) ?? { count: 0, onlyInFiles: true };
          slot.count += 1;
          slot.onlyInFiles &&= !(app.get(path)?.tags ?? []).includes(name);
          found.set(name, slot);
        }
      }
      return {
        rev,
        tags: [...found]
          .map(([name, v]) => ({
            name,
            count: v.count,
            onlyInFiles: v.onlyInFiles,
            ...mark(name, null),
          }))
          .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name)),
        untagged: documents().filter((p) => tagsOf(p).length === 0).length,
        orphans: [...app.keys()].filter((p) => !project.files.has(p)).length,
        marks: projects().view?.vocabulary.active ?? false,
      };
    },
    "tags.status": () => status(),
    "tags.filesWith": ({ tags, untagged }) =>
      universe()
        .filter((path) =>
          untagged === true
            ? MARKDOWN.test(path) && tagsOf(path).length === 0
            : tags.length > 0 && tags.every((t) => tagsOf(path).includes(t)),
        )
        .map((path) => ({ path, description: app.get(path)?.description ?? "", modifiedMs: 0 })),
    "tags.fileMeta": ({ path }) => meta(path),
    "tags.setTags": ({ path, tags }) => {
      refuse();
      if (!project.files.has(path)) throw new MockPluginError("notFound", path);
      put(path, { tags: normalizeTags(tags), description: app.get(path)?.description ?? "" });
      return meta(path);
    },
    "tags.setDescription": ({ path, description }) => {
      refuse();
      if (!project.files.has(path)) throw new MockPluginError("notFound", path);
      put(path, {
        tags: app.get(path)?.tags ?? [],
        description: normalizeDescription(description),
      });
      return meta(path);
    },
    "tags.setStyle": ({ tag, color, icon }) => {
      refuse();
      const style = {
        color: COLORS.find((c) => c === color) ?? null,
        icon: ICONS.find((i) => i === icon) ?? null,
      };
      if (style.color === null && style.icon === null) styles.delete(tag);
      else styles.set(tag, style);
      changed();
      return null;
    },
    "tags.orphans": () => [...app.keys()].filter((p) => !project.files.has(p)).sort(),
    "tags.cleanUp": () => {
      refuse();
      const gone = [...app.keys()].filter((p) => !project.files.has(p));
      for (const path of gone) app.delete(path);
      if (gone.length > 0) changed();
      return gone.length;
    },
    "comments.list": ({ path, scope }) =>
      stored
        .filter((c) => path === undefined || path === null || c.path === path)
        .map(view)
        .filter((c) => (scope ?? "open") === "all" || (scope === "done" ? c.done : !c.done)),
    "comments.add": ({ path, quote, text, headingPath, line, endLine }) => {
      if (!project.files.has(path)) throw new MockPluginError("notFound", path);
      if (fold(quote) === "" || text.trim() === "")
        throw new MockPluginError("invalidData", "text");
      const entry: Stored = {
        id: `c${next++}`,
        path,
        text: text.trim(),
        quote: quote.trim(),
        headingPath,
        line: Math.max(1, line),
        endLine: Math.max(1, endLine),
        createdMs: Date.now(),
        doneMs: null,
        replies: [],
      };
      stored.push(entry);
      emit("comments:changed", null);
      return view(entry);
    },
    "comments.setDone": ({ id, done }) => {
      const c = stored.find((s) => s.id === id);
      if (!c) throw new MockPluginError("notFound", id);
      c.doneMs = done ? Date.now() : null;
      emit("comments:changed", null);
      return null;
    },
    "comments.remove": ({ id }) => {
      const at = stored.findIndex((s) => s.id === id);
      if (at < 0) throw new MockPluginError("notFound", id);
      stored.splice(at, 1);
      emit("comments:changed", null);
      return null;
    },
    "comments.reply": ({ id, text }) => {
      const c = stored.find((s) => s.id === id);
      if (!c) throw new MockPluginError("notFound", id);
      c.replies.push({ atMs: Date.now(), by: "user", text: text.trim() });
      emit("comments:changed", null);
      return null;
    },
    "quiet.rules": () => ({ marks: { ...marks } }),
    "quiet.set": ({ paths, interest }) => {
      const next: Record<string, Interest> = { ...marks };
      for (const path of paths) next[path] = interest;
      // a "speaking" mark with nothing silenced above it changes nothing
      for (const [path, mark] of Object.entries(next)) {
        if (mark !== "speaking") continue;
        const { [path]: _own, ...without } = next;
        if (!isQuiet(path, without)) delete next[path];
      }
      marks = next;
      emit("quiet:changed", { marks: { ...marks } });
      return { marks: { ...marks } };
    },
    "broken-links.list": () => broken(),
  };

  return {
    handlers,
    allows: (options) => (path) =>
      (options.tags.length === 0 || options.tags.every((t) => tagsOf(path).includes(t))) &&
      (options.includeSilenced || !isQuiet(path, marks)),
    probe: {
      tagsProblem: (next) => {
        problem = next;
        changed();
      },
      appTags: (path) => [...(app.get(path)?.tags ?? [])],
      comments: () => stored.map(view),
      marks: () => ({ ...marks }),
      reset: () => {
        app.clear();
        styles.clear();
        stored.length = 0;
        marks = {};
        problem = null;
        changed();
        emit("comments:changed", null);
        emit("quiet:changed", { marks: {} });
      },
    },
  };
}
