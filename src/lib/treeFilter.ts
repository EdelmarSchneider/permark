// The Explorer filter: which files of the loaded tree are shown. Pure logic, no UI: presets are bundles of
// file categories (see fileKinds.ts), "custom" adds gitignore-style `+pattern` / `-pattern` entries, and
// folders left with nothing to show can be dropped. Two switches sit on top of every preset: hidden files
// (names that start with a dot) and files that version control ignores.

import { FILE_CATEGORIES, categoryOf, isFileCategory, type FileCategory } from "./fileKinds";
import { WT_PREFIX, dirname, splitWorktree } from "./paths";
import { isDirNode } from "./tree";
import type { FilterShow, TreeDefault, TreeFilter, TreeNode } from "./types";

export const FILTER_SHOWS = [
  "documents",
  "docs-and-assets",
  "everything",
  "custom",
] as const satisfies readonly FilterShow[];

const PRESET_CATEGORIES: Record<Exclude<FilterShow, "custom">, readonly FileCategory[]> = {
  documents: ["documents", "text"],
  "docs-and-assets": ["documents", "text", "images", "data", "xml"],
  everything: FILE_CATEGORIES,
};

/** The categories a preset shows; "custom" starts from nothing. */
export function categoriesOf(show: FilterShow): FileCategory[] {
  return show === "custom" ? [] : [...PRESET_CATEGORIES[show]];
}

export function isFilterShow(value: unknown): value is FilterShow {
  return typeof value === "string" && (FILTER_SHOWS as readonly string[]).includes(value);
}

export const DEFAULT_TREE_FILTER: TreeFilter = {
  show: "docs-and-assets",
  categories: categoriesOf("docs-and-assets"),
  patterns: "",
  hideEmptyFolders: true,
  assetsNearDocs: true,
  showIgnored: false,
  showHidden: false,
};

/** What the backend has to load for a filter: the filter itself runs over the tree that comes back. */
export type TreeLoad = { showIgnored: boolean; showHidden: boolean };

export function treeLoadOf(filter: TreeFilter | null): TreeLoad {
  return { showIgnored: filter?.showIgnored === true, showHidden: filter?.showHidden === true };
}

export function sameTreeLoad(a: TreeLoad, b: TreeLoad): boolean {
  return a.showIgnored === b.showIgnored && a.showHidden === b.showHidden;
}

/**
 * The filter a project asks for. Patterns turn a preset into "custom" with that preset's categories
 * checked, so every active rule is visible in the filter popover.
 */
export function filterFromDefault(config: TreeDefault | null): TreeFilter {
  const show = config?.show ?? DEFAULT_TREE_FILTER.show;
  const patterns = [
    ...(config?.include ?? []).map((p) => `+${p}`),
    ...(config?.exclude ?? []).map((p) => `-${p}`),
  ].join(" ");
  if (show === "custom" || patterns !== "") {
    return { ...DEFAULT_TREE_FILTER, show: "custom", categories: categoriesOf(show), patterns };
  }
  return { ...DEFAULT_TREE_FILTER, show, categories: categoriesOf(show) };
}

/** Switching to "custom" keeps what the previous preset showed, so the tree does not jump. */
export function withShow(filter: TreeFilter, show: FilterShow): TreeFilter {
  if (show === filter.show) return filter;
  const categories =
    show === "custom" && filter.show !== "custom" ? categoriesOf(filter.show) : filter.categories;
  return { ...filter, show, categories };
}

/** A stored filter, checked field by field. Null when it is missing or does not name a known preset. */
export function normalizeTreeFilter(raw: unknown): TreeFilter | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  if (!isFilterShow(r.show)) return null;
  const categories = Array.isArray(r.categories)
    ? [...new Set(r.categories.filter(isFileCategory))]
    : [];
  return {
    show: r.show,
    categories,
    patterns: typeof r.patterns === "string" ? r.patterns : "",
    hideEmptyFolders: r.hideEmptyFolders !== false,
    assetsNearDocs: r.assetsNearDocs !== false,
    showIgnored: r.showIgnored === true,
    showHidden: r.showHidden === true,
  };
}

// ---- patterns ------------------------------------------------------------------------------------

export type PatternRule = { include: boolean; source: string; regex: RegExp };
export type ParsedPatterns = { rules: PatternRule[]; invalid: string[] };

function escapeRegex(ch: string): string {
  return /[\\^$.*+?()[\]{}|/-]/.test(ch) ? `\\${ch}` : ch;
}

/** The regex source of a glob (`*`, `**`, `?`, `[abc]`, `{a,b}`), or null when it is malformed. */
function globSource(glob: string): string | null {
  let out = "";
  let braces = 0;
  for (let i = 0; i < glob.length; i++) {
    const ch = glob[i];
    if (ch === "*") {
      let end = i;
      while (glob[end + 1] === "*") end++;
      if (end === i) {
        out += "[^/]*";
      } else if ((i === 0 || glob[i - 1] === "/") && glob[end + 1] === "/") {
        out += "(?:.*/)?"; // `**/` also matches nothing, so `**/a` finds a top-level `a`
        end++;
      } else {
        out += ".*";
      }
      i = end;
    } else if (ch === "?") {
      out += "[^/]";
    } else if (ch === "{") {
      braces++;
      out += "(?:";
    } else if (ch === "}") {
      if (braces === 0) return null;
      braces--;
      out += ")";
    } else if (ch === "," && braces > 0) {
      out += "|";
    } else if (ch === "[") {
      const close = glob.indexOf("]", i + 2);
      if (close < 0) return null;
      let body = glob.slice(i + 1, close);
      const negated = body[0] === "!" || body[0] === "^";
      if (negated) body = body.slice(1);
      if (body === "") return null;
      out += `[${negated ? "^" : ""}${body.replace(/[\\\]^]/g, "\\$&")}]`;
      i = close;
    } else if (ch === "\\") {
      i++;
      if (i >= glob.length) return null;
      out += escapeRegex(glob[i]);
    } else {
      out += escapeRegex(ch);
    }
  }
  return braces === 0 ? out : null;
}

/**
 * One gitignore-style pattern as a regex over a project-relative path. Without a `/` it matches a name
 * at any depth; with one it is anchored at the project root; a trailing `/` means a folder's contents.
 * A pattern that names a folder also covers everything inside it.
 */
function patternRegex(pattern: string): RegExp | null {
  const dirOnly = pattern.endsWith("/");
  let glob = pattern.replace(/\/+$/, "");
  const anchored = glob.includes("/");
  glob = glob.replace(/^\/+/, "");
  if (glob === "") return null;
  const source = globSource(glob);
  if (source === null) return null;
  try {
    return new RegExp(
      `^${anchored ? "" : "(?:.*/)?"}(?:${source})${dirOnly ? "/.*" : "(?:/.*)?"}$`,
      "i",
    );
  } catch {
    return null; // e.g. a reversed range like [z-a]
  }
}

/** Splits the advanced field at white space and at commas outside `{...}`. */
export function splitPatterns(text: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let depth = 0;
  for (const ch of text) {
    if (ch === "{") depth++;
    else if (ch === "}") depth = Math.max(0, depth - 1);
    if (/\s/.test(ch) || (ch === "," && depth === 0)) {
      if (current !== "") tokens.push(current);
      current = "";
    } else {
      current += ch;
    }
  }
  if (current !== "") tokens.push(current);
  return tokens;
}

/** `+pattern` shows files, `-pattern` hides them (a bare pattern shows). Malformed entries are listed in `invalid`. */
export function parsePatterns(text: string): ParsedPatterns {
  const rules: PatternRule[] = [];
  const invalid: string[] = [];
  for (const token of splitPatterns(text)) {
    const include = token[0] !== "-";
    const body = token[0] === "+" || token[0] === "-" ? token.slice(1) : token;
    const regex = patternRegex(body);
    if (regex) rules.push({ include, source: token, regex });
    else invalid.push(token);
  }
  return { rules, invalid };
}

// ---- deciding what is shown --------------------------------------------------------------------

/** True when the file is shown. Takes the path as it appears in the tree (worktree files carry `@wt/<name>/`). */
export type FileTest = (path: string) => boolean;

/** A name that starts with a dot (`.github`, `.env`, `.gitignore`). */
export function isHiddenName(name: string): boolean {
  return name.startsWith(".");
}

/**
 * Whether the path has a hidden name in it, below the root it belongs to: a worktree is judged by the
 * path inside it, never by its own folder name (a worktree under `.claude/` is still a root of its own).
 */
export function isHiddenPath(path: string): boolean {
  return splitWorktree(path).rel.split("/").some(isHiddenName);
}

/** The categories that count as documents: a folder that holds one is a place for reading. */
const DOCUMENT_CATEGORIES: ReadonlySet<FileCategory> = new Set(["documents", "text"]);
/** The companions of documents that "only assets next to documents" keeps out of code folders. */
const ASSET_CATEGORIES: ReadonlySet<FileCategory> = new Set(["images", "data", "xml"]);

/** Every folder that directly holds a document (`""` is the project root, `@wt/<name>` a worktree root). */
export function documentFolders(files: Iterable<string>): Set<string> {
  const folders = new Set<string>();
  for (const path of files) {
    if (DOCUMENT_CATEGORIES.has(categoryOf(path))) folders.add(dirname(path));
  }
  return folders;
}

/**
 * Whether a document sits next to the file: in its own folder, or in the folder above it. The project
 * root (and a worktree's root) counts as the file's own folder only: `package.json` sits next to
 * `README.md`, but `src-tauri/Cargo.toml` and `.vscode/settings.json` do not borrow the root's.
 */
function nearDocuments(path: string, folders: ReadonlySet<string>): boolean {
  const own = dirname(path);
  if (folders.has(own)) return true;
  return splitWorktree(own).rel.includes("/") && folders.has(dirname(own));
}

/** `folders` comes from `documentFolders` over the files of the tree. */
export function fileTest(filter: TreeFilter, folders: ReadonlySet<string>): FileTest {
  const visible: FileTest = filter.showHidden ? () => true : (path) => !isHiddenPath(path);
  if (filter.show === "everything") return visible;
  const categories = new Set<FileCategory>(
    filter.show === "custom" ? filter.categories : PRESET_CATEGORIES[filter.show],
  );
  const near = filter.assetsNearDocs && filter.show !== "documents";
  const rules = filter.show === "custom" ? parsePatterns(filter.patterns).rules : [];
  const byCategory = (path: string): boolean => {
    if (!visible(path)) return false;
    const category = categoryOf(path);
    if (!categories.has(category)) return false;
    return !near || !ASSET_CATEGORIES.has(category) || nearDocuments(path, folders);
  };
  if (rules.length === 0) return byCategory;
  return (path) => {
    // patterns mean the same in a worktree as in the main folder
    const relative = path.startsWith(WT_PREFIX) ? splitWorktree(path).rel : path;
    let shown = byCategory(path);
    for (const rule of rules) if (rule.regex.test(relative)) shown = rule.include; // the last match wins
    return shown;
  };
}

/** Whether the node is a file, or a folder with a file somewhere inside. */
function holdsFile(node: TreeNode): boolean {
  return !isDirNode(node) || (node.children ?? []).some(holdsFile);
}

export type FilterOutcome = {
  main: TreeNode[];
  worktrees: TreeNode[];
  /** Files that pass the filter. */
  shown: string[];
  /** Files the filter hides (a hidden file that is open still appears in the tree: see `forced`). */
  hidden: string[];
  /** Hidden files kept in the tree because they are open. */
  forced: Set<string>;
};

/**
 * The tree reduced to the files that pass `test`. With `hideEmptyFolders`, folders (and worktree roots)
 * with no file left are dropped. `keep` is the open file: it stays in the tree even when hidden.
 */
export function filterTree(
  root: TreeNode | null,
  test: FileTest,
  options: {
    hideEmptyFolders: boolean;
    keep: string | null;
    /** Folders made in the Explorer this session: they stay while they are empty, waiting for their files. */
    keepFolders?: ReadonlySet<string>;
    /** Hidden folders (names that start with a dot) stay out unless one holds a file that is shown. */
    showHidden?: boolean;
  },
): FilterOutcome {
  const out: FilterOutcome = { main: [], worktrees: [], shown: [], hidden: [], forced: new Set() };
  if (!root) return out;

  const prune = (nodes: TreeNode[]): TreeNode[] => {
    const kept: TreeNode[] = [];
    for (const node of nodes) {
      if (isDirNode(node)) {
        const children = prune(node.children ?? []);
        if (node.kind === "dir" && !options.showHidden && isHiddenName(node.name)) {
          // a hidden folder is not listed for being there, only for a file in it that is (a pattern, the open file)
          if (children.some(holdsFile)) kept.push({ ...node, children });
          continue;
        }
        if (
          children.length > 0 ||
          !options.hideEmptyFolders ||
          node.large !== undefined ||
          (options.keepFolders?.has(node.path) && (node.children ?? []).length === 0)
        ) {
          kept.push({ ...node, children });
        }
      } else if (test(node.path)) {
        out.shown.push(node.path);
        kept.push(node);
      } else {
        out.hidden.push(node.path);
        if (node.path === options.keep) {
          out.forced.add(node.path);
          kept.push(node);
        }
      }
    }
    return kept;
  };

  const top = prune(root.children ?? []);
  out.main = top.filter((n) => n.kind !== "worktree");
  out.worktrees = top.filter((n) => n.kind === "worktree");
  return out;
}
