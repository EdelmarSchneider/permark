// Mirrors the Rust <-> UI contract defined by the Tauri commands in src-tauri/src/commands.rs.
// Paths are vault-relative, `/`-separated, real on-disk case. Worktree paths are prefixed with `@wt/<name>/`.

export type VaultInfo = {
  root: string;
  name: string;
  mode: "ide" | "simple";
  initialFile: string | null;
  /** `branch` is the checked-out branch, `null` when detached or unreadable. */
  worktrees: { name: string; path: string; branch: string | null }[];
};
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
};
