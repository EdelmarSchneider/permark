import type { FileCategory } from "./fileKinds";

// Mirrors the Rust <-> UI contract defined by the Tauri commands in src-tauri/src/commands.rs.
// Paths are vault-relative, `/`-separated, real on-disk case. Worktree paths are prefixed with `@wt/<name>/`.

export type VaultInfo = {
  root: string;
  name: string;
  mode: "ide" | "simple";
  initialFile: string | null;
  /** `branch` is the checked-out branch, `null` when detached or unreadable. */
  worktrees: { name: string; path: string; branch: string | null }[];
  /** False when the file system cannot report changes (some network or virtual drives): the folder
   *  works, but is only refreshed when the window gets focus. */
  liveUpdates: boolean;
  /** The project's default for the Explorer filter (`[tree]` in `.trailmark/config.toml`), if it sets one. */
  treeDefault: TreeDefault | null;
};
/** The presets of the Explorer filter (`show` in `[tree]` of `config.toml` uses the same names). */
export type FilterShow = "documents" | "docs-and-assets" | "everything" | "custom";
/** A person's Explorer filter for a project. */
export type TreeFilter = {
  show: FilterShow;
  /** What "custom" shows (the presets ignore it). */
  categories: FileCategory[];
  /** The advanced field of "custom": `+pattern` and `-pattern` entries (the presets ignore it). */
  patterns: string;
  hideEmptyFolders: boolean;
  /** Images, data and XML files show only in folders that hold documents (see `treeFilter.ts`). */
  assetsNearDocs: boolean;
};
/** The project's default for the Explorer filter, from `[tree]` in `.trailmark/config.toml`. */
export type TreeDefault = { show: FilterShow | null; include: string[]; exclude: string[] };
export type RecentVault = { root: string; name: string; lastOpenedMs: number };
export type TreeNode = {
  name: string;
  path: string;
  kind: "dir" | "file" | "worktree" | "link";
  children?: TreeNode[];
};
export type DocumentView = {
  path: string;
  kind: "markdown" | "json" | "xml" | "text" | "binary" | "tooLarge";
  html: string;
  blocks: string[] /* per top-level block hash, for the LCS diff */;
  title: string | null;
  frontmatter: [string, string][] | null;
  modifiedMs: number;
  sizeBytes: number;
  /** A short remark shown next to the title: an odd encoding, or a rendering fallback. */
  encodingNote: string | null;
};
export type LinkTarget =
  | { kind: "internal"; path: string; anchor: string | null }
  | { kind: "external"; url: string }
  | { kind: "anchor"; anchor: string }
  | { kind: "missing"; path: string };
export type GitState = "modified" | "added" | "untracked" | "deleted" | "renamed" | "conflicted";
export type GitSnapshot = {
  generation: number;
  isRepo: boolean;
  gitError: string | null;
  head: { branch: string | null; oid: string | null; detached: boolean; unborn: boolean };
  repoState: "clean" | "merge" | "rebase" | "cherryPick" | "revert" | "bisect" | "other";
  baseline: { kind: "lastVisit" | "head"; oid: string | null };
  files: Record<string, { state: GitState; staged: boolean; from?: string }>;
  truncated: boolean;
};
export type FsBatch = {
  changes: FsChange[];
  bulk: null | {
    count: number;
    reason: "gitOperation" | "large" | "overflow";
    paths: string[] /* capped at 200 */;
  };
  rescan: boolean;
};
export type FsChange = {
  kind: "created" | "modified" | "removed" | "renamed";
  path: string;
  from?: string;
  atMs: number;
};
export type ChangedSince = {
  sinceMs: number | null;
  files: { path: string; state: GitState | "modified"; modifiedMs: number }[];
};
export type SearchHit = {
  path: string;
  headingPath: string[];
  snippet: string /* sanitized, <mark> */;
  startLine: number;
  score: number;
};
export type SessionState = {
  lastDoc: string | null;
  scrollTop: number;
  zoom: number;
  wide: boolean;
  refTemplate: string;
  leftPanel: { open: boolean; width: number };
  rightPanel: { open: boolean; width: number };
  theme: "system" | "light" | "dark";
  follow: boolean;
  /** The Explorer filter this person chose for the project; null until they do (the project's default applies). */
  treeFilter: TreeFilter | null;
};

// ---- tags and descriptions (docs/design/v0.2.md section 1) ---------------------------------------
/** One tag of a file. A tag in both places is listed once, as a file tag (`file`); `app` says the app has it too. */
export type TagRef = { name: string; file: boolean; app: boolean };
/** The tags and description of one file. `writable` is false when the app's tags cannot be changed now. */
export type FileMeta = {
  path: string;
  tags: TagRef[];
  description: string;
  writable: boolean;
};
/** Why `meta.toml` could not be read (a merge conflict, a bad edit). */
export type MetaProblem = { message: string; line: number | null };
export type MetaStatus = {
  problem: MetaProblem | null;
  /** The last write of `meta.toml` failed (it is tried again). */
  writeError: string | null;
  /** Changes made before `meta.toml` became unreadable: kept, and saved once it can be read. */
  pendingChanges: number;
};
export type TagCount = { name: string; count: number; onlyInFiles: boolean };
export type TagOverview = {
  tags: TagCount[];
  untagged: number;
  /** Entries kept for files that no longer exist. */
  missing: number;
  status: MetaStatus;
};
export type TaggedFile = { path: string; description: string; modifiedMs: number };
/** Tags and description of a file that has some (for filters and tooltips). `tags` merges both sources. */
export type IndexEntry = { path: string; tags: string[]; description: string; modifiedMs: number };
