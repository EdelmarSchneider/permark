import type { FileCategory } from "./fileKinds";
import type { ReadingWidth } from "./readingWidth";
import type { EditMode } from "./editMode";
import type { RightTab } from "./rightPanel";

// Mirrors the Rust <-> UI contract defined by the Tauri commands in src-tauri/src/commands.rs.
// Paths are vault-relative, `/`-separated, real on-disk case. Worktree paths are prefixed with `@wt/<name>/`.

export type VaultInfo = {
  root: string;
  name: string;
  /** What the folder is for: a project, or the notes folder of the Notes window. */
  kind: "project" | "notes";
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
  /** Also list the files that version control ignores (the tree is loaded without ignore rules; see `vault.refreshTree`). */
  showIgnored: boolean;
  /** Also list hidden files and folders (names that start with a dot). Applies to every preset. */
  showHidden: boolean;
};
/** The project's default for the Explorer filter, from `[tree]` in `.trailmark/config.toml`. */
export type TreeDefault = { show: FilterShow | null; include: string[]; exclude: string[] };
export type RecentVault = { root: string; name: string; lastOpenedMs: number };
/**
 * Which build of the app this is: the one the project publishes ("standard"), or the one in the Microsoft Store
 * ("store"), which the Store updates, so it has no "Check for updates".
 */
export type Distribution = "standard" | "store";
/** What "Open with TrailMark" has registered with the file manager, as the system has it right now. */
export type ShellState = {
  /** False where TrailMark cannot register anything (every platform but Windows, for now). */
  supported: boolean;
  /** TrailMark is in the right-click menu of folders and Markdown files. */
  contextMenu: boolean;
  /** TrailMark is in the "Open with" list of Markdown files. */
  association: boolean;
  /** Windows' own choice for `.md` files is TrailMark. */
  isDefault: boolean;
};
export type TreeNode = {
  name: string;
  path: string;
  kind: "dir" | "file" | "worktree" | "link";
  children?: TreeNode[];
  /** An ignored folder that was too large to load: it holds more than this many entries and shows no children. */
  large?: number;
};
export type DocumentView = {
  path: string;
  kind: "markdown" | "json" | "xml" | "text" | "binary" | "tooLarge" | "image" | "audio" | "video";
  html: string;
  blocks: string[] /* per top-level block hash, for the LCS diff */;
  title: string | null;
  frontmatter: [string, string][] | null;
  modifiedMs: number;
  sizeBytes: number;
  /** Codes for what to tell the reader next to the title (an odd encoding, a rendering fallback), several joined with a comma; `noteText` words them. */
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
  width: ReadingWidth;
  /** The on/off width of sessions saved before v0.2. Only read (and migrated to `width`); never written. */
  wide?: boolean;
  refTemplate: string;
  leftPanel: { open: boolean; width: number };
  rightPanel: { open: boolean; width: number };
  theme: "system" | "light" | "dark";
  follow: boolean;
  /** The Explorer filter this person chose for the project; null until they do (the project's default applies). */
  treeFilter: TreeFilter | null;
  /** The Explorer selects and scrolls to the document that was just opened (on unless turned off). */
  autoReveal: boolean;
  /** The view the right panel shows: the feed or the document outline. */
  rightTab: RightTab;
  /** Whether moving files in the Explorer rewrites the links to them. */
  linkUpdate: LinkUpdate;
  /** How a document opens for editing. */
  editMode: EditMode;
  /** "Never for this folder" was chosen on the hint to keep settings with the project. */
  initHintNever: boolean;
};

// ---- the agent connection (docs/design/v0.4.md) ------------------------------------------------------

/** How the open document is shown. */
export type AgentViewMode = "reading" | "visual" | "source";

/** What the window publishes for an agent that asks: the document, the section on screen and the selection. */
export type AgentContext = {
  /** The open document (a path of the project), or null. */
  document: string | null;
  mode: AgentViewMode | null;
  /** The section on screen: its heading path and its lines (from 1, both included). */
  section: { headingPath: string[]; startLine?: number; endLine?: number } | null;
  /** The selected text; the lines are missing when the editor cannot tell. */
  selection: {
    text: string;
    headingPath: string[];
    startLine?: number;
    endLine?: number;
  } | null;
  /** The editor holds changes that are not saved to the file. */
  unsaved: boolean;
};

/** One request an agent made. */
export type AgentRequest = {
  /** The tool: `get_context`, `list_unread`, `search` or `open`. */
  tool: string;
  /** What it was about (the query, the path). */
  subject: string | null;
  atMs: number;
};

/** Why the agent connection could not start; the interface words each kind itself (`agentFailureText`). */
export type AgentFailureKind = "inUse" | "folderNotPrivate" | "pathTooLong" | "other";

/** A connection that was meant to listen and could not. */
export type AgentFailure = {
  kind: AgentFailureKind;
  /** The system's own words, in English (the place and the error): for a tooltip, and the text of a kind the interface has no words for. */
  message: string;
};

/** The agent connection of the open project. */
export type AgentStatus = {
  enabled: boolean;
  /** The window is listening (not when the socket could not be opened; `error` says why). */
  listening: boolean;
  /** Programs connected now (an agent's `trailmark mcp`, a hook's `trailmark context`). */
  connections: number;
  /** The latest requests, newest first. */
  recent: AgentRequest[];
  error: AgentFailure | null;
  /** The program an agent starts (`<program> mcp`): where this one is, for the setup lines; null when unknown. */
  program: string | null;
};

/** A place an agent asked to be shown. */
export type AgentOpen = {
  path: string;
  /** The line to show (from 1): the one asked for, or the one the heading is on. */
  line?: number;
  /** A heading was asked for and the document has none like it. */
  headingNotFound?: boolean;
};

// ---- comments (docs/design/v0.4.md section 3) ---------------------------------------------------

/** Whether a comment's passage could be found again in the document. */
export type CommentPassage = "anchored" | "passageChanged" | "documentGone";

/** A comment with its place worked out against the document as it is now. */
export type Comment = {
  id: string;
  /** The document (a path of the project). */
  path: string;
  /** What the person wrote. */
  text: string;
  /** The passage the comment is on, as it was selected. */
  quote: string;
  /** The headings above the passage, its own section last. */
  headingPath: string[];
  /** Where the passage is now (lines from 1); the best guess unless `state` is `anchored`. */
  line: number;
  endLine: number;
  createdMs: number;
  state: CommentPassage;
  done: boolean;
  doneMs?: number;
  doneBy?: "person" | "agent";
  /** A short answer from the agent that marked the comment done. */
  reply?: string;
};

/** A comment the person just wrote: the passage as the reading view knows it. */
export type NewComment = {
  path: string;
  quote: string;
  text: string;
  headingPath: string[];
  line: number;
  endLine: number;
};

// ---- editing (docs/design/v0.2.md section 3) -------------------------------------------------------
/** What a top-level block is; decides what the Visual editor may do with it (locked blocks keep their bytes). */
export type BlockKind =
  | "paragraph"
  | "heading"
  | "list"
  | "table"
  | "code"
  | "html"
  | "frontmatter"
  | "math"
  | "footnoteDef"
  | "thematicBreak"
  | "blockquote"
  | "other";
/** A top-level block as a range of UTF-8 bytes of the text it was parsed from. */
export type BlockInfo = { startByte: number; endByte: number; kind: BlockKind };
/** The line ending of a file; "mixed" when it uses several. */
export type Eol = "lf" | "crlf" | "mixed";
export type ParsedBlocks = { eol: Eol; blocks: BlockInfo[] };
/** A Markdown file as the editor needs it: the exact text on disk (a BOM is its first character), the hash of its bytes, its blocks. */
export type SourceDoc = ParsedBlocks & {
  path: string;
  text: string;
  hash: string;
  modifiedMs: number;
};
/** Saved, or the file is no longer the version the editor started from (`diskHash` null: it is gone). */
export type SaveOutcome =
  | { result: "saved"; hash: string; modifiedMs: number }
  | { result: "conflict"; diskHash: string | null };
export type CreatedDoc = { path: string; hash: string; modifiedMs: number };
/** An image written next to a document: `rel` is what the Markdown links to, `path` the vault path. */
export type AssetWritten = { path: string; rel: string; bytes: number };

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

// ---- file operations (docs/design/v0.2.md section 17) -------------------------------------------
/** What happens to the links of documents when files are moved or renamed in the Explorer. */
export type LinkUpdate = "ask" | "always" | "never";
/** A file or folder that moves (a rename is a move within its folder). */
export type Move = { from: string; to: string };
/** Why a document's links were left alone (or an undo could not restore it). */
export type SkipReason =
  | "tooLarge"
  | "notUtf8"
  | "unreadable"
  | "changed"
  | "writeFailed"
  | "missing"
  | "occupied"
  | "unsavedVisual";
export type SkippedFile = { path: string; reason: SkipReason };
/** One link of a document that would change: `old` and `new` are its destination as written. */
export type PreviewChange = { line: number; old: string; new: string };
export type PreviewFile = { path: string; movedTo: string | null; changes: PreviewChange[] };
/** What a move would do: the checked moves and the links that would change (the list is cut off at a few thousand). */
export type MovePreview = {
  planId: number;
  moves: Move[];
  linkCount: number;
  fileCount: number;
  files: PreviewFile[];
  truncated: boolean;
  skipped: SkippedFile[];
};
/** The text of a document being edited, with changes that are not saved yet. */
export type BufferText = { path: string; text: string };
/** A change to the text of an editor: UTF-16 offsets. */
export type TextEdit = { from: number; to: number; insert: string };
/** The changes for the unsaved text of a document; `newPath` is where the document is after the move. */
export type BufferEdits = { path: string; newPath: string; edits: TextEdit[] };
export type MoveOutcome = {
  /** For `undoFileOp`. */
  opId: number;
  moved: Move[];
  filesUpdated: number;
  linksUpdated: number;
  buffers: BufferEdits[];
  skipped: SkippedFile[];
};
export type DeleteOutcome = { deleted: string[] };
export type CreatedFolder = { opId: number; path: string };
export type UndoOutcome = {
  movedBack: Move[];
  filesRestored: number;
  linksRestored: number;
  foldersRemoved: number;
  couldNot: SkippedFile[];
};

// ---- notes ----------------------------------------------------------------------------------------

/** Where TrailMark opens when it is started with nothing to open. */
export type StartIn = "welcome" | "notes";
/** The notes folder: where it is, and whether it is there. */
export type NotesFolder = {
  path: string;
  /**
   * "missing": a folder the person chose that is not there (an unplugged drive, a renamed folder). They can locate it or create it;
   * it is never made quietly. The default folder is never missing: opening the notes makes it, without asking.
   */
  status: "ready" | "missing";
  /** The default folder, not one the person chose. */
  isDefault: boolean;
};
/** The editor a note was left in, as the notes remember it. */
export type NoteMode = "read" | "visual" | "source";
/** Where a note was left: the editor, the caret (a character offset) and the scroll position. */
export type NoteView = {
  mode: NoteMode | null;
  caret: number | null;
  scroll: number | null;
};
/**
 * A row of the list: a note (a file of the notes folder) or a loose file (one opened here that is somewhere else). A note's
 * `path` is its file name in the notes folder; a loose file's is its path for the window, `@file/<id>/<name>`, which holds an
 * id and never a place of the disk.
 */
export type NoteEntry = {
  kind: "note" | "file";
  path: string;
  /** The first line without its Markdown marks; empty for a note with no text. A loose file shows its name. */
  title: string;
  snippet: string;
  modifiedMs: number;
  sizeBytes: number;
  pinned: boolean;
  /** The note was copied and not edited since. */
  copied: boolean;
  view: NoteView | null;
  /** A loose file: the folder it is in, as shown in the list (`~\Downloads`). */
  folder: string | null;
  /** A loose file that is not where it was. */
  missing: boolean;
  /** A loose file with text that was typed and not saved. */
  unsaved: boolean;
  /** A note the person shared with agents (see `list_notes` and `read_note` of the agent connection). */
  shared: boolean;
};
/** What the Save dialog starts with when a note is moved into a project. */
export type MoveSuggestion = {
  /** Where the dialog opens, with the file name made from the note's title. */
  defaultPath: string;
  /** The project it opens in: the one opened last, when it is still there. */
  folder: string | null;
};
/** A note that was moved into a project: where its copy is, the row it became, and how to ask for the note back. */
export type NoteMoved = { destination: string; file: string; undoId: number | null };
/** What Undo of a move did: the note that is back, and whether the copy in the project was taken away (not when it was changed). */
export type MoveUndone = { note: string; removed: boolean };
/** What opening a file made of it: a note when it is one of the notes folder, else a loose file. `path` is what to open. */
export type OpenedFile = { kind: "note" | "file"; path: string };
/** How a loose file looks on disk compared with what was last read: the same, changed since, or gone. */
export type LooseState = "same" | "changed" | "missing";
export type LooseStatus = {
  path: string;
  state: LooseState;
  /** How the file looks now (its size and time), to tell one change from the next. */
  stamp: string;
  /** The path the file had before it was found again under another name in its folder. */
  was: string | null;
};
/** The text typed into a loose file and not saved, with the hash of the version of the file it was typed over. */
export type PendingEdits = { path: string; baseHash: string; text: string; savedMs: number };
/** What a path of the disk is, and where it is in the project the window shows. */
export type PathClass = { kind: "folder" | "file" | "missing"; inProject: string | null };
/** The notes, the pinned ones first in the person's order, then the rest by last change. */
export type NoteList = { notes: NoteEntry[]; open: string | null };
/** What the welcome screen shows of the notes. */
export type NotesSummary = { pinned: NoteEntry[]; latest: NoteEntry[] };
export type NotesOverview = { folder: NotesFolder; summary: NotesSummary };
/** Why the notes can only be read in this window. */
export type NotesReadOnly = "locked" | "unwritable";
/** What opening the notes found. */
export type NotesOpened = {
  info: VaultInfo;
  readOnly: NotesReadOnly | null;
  /** Notes made from text that a crash left in temporary files. */
  recovered: string[];
};
/** A new note: an empty file, already in the list. `hash` is the hash of that file, which the first save of its text is guarded with. */
export type NoteCreated = { path: string; hash: string; modifiedMs: number };
/**
 * The result of saving a note. "saved": `path` is the file's name, which differs from the one asked for only when the note had
 * been deleted by another program and its name was taken since (`created` is then true). "conflict": the note changed since it
 * was opened; nothing of the other version was overwritten, and the text being saved is in the note `copy`.
 */
export type NoteSaved =
  | { result: "saved"; path: string; hash: string; modifiedMs: number; created: boolean }
  | { result: "conflict"; copy: string };
/** An earlier version of a note. */
export type NoteVersion = { id: string; writtenMs: number; sizeBytes: number };
/** Notes sent to the recycle bin; `undoId` asks for them back (null when they are too big to keep for it). */
export type NoteDeleted = { deleted: string[]; undoId: number | null };
export type NoteRestored = { hash: string; modifiedMs: number };
/** What another window or launch asked the Notes window for: a new note, a note, or a file of the disk. */
export type NotesRequest = { newNote: boolean; open: string | null; openFile: string | null };
/** What the program is. */
export type LaunchMode = { notes: boolean };
/**
 * What became of a window that showed only the welcome screen when it asked for the Notes window. "became": this window is the
 * Notes window now (same window, same place). "forwarded": a Notes window already runs in another program and was asked to come
 * forward, so this window is to close.
 */
export type NotesHandoff = "became" | "forwarded";
