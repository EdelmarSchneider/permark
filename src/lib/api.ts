// The ONLY module that talks to the backend. Everything else in the UI imports `api` from here.
// Command names are the snake_case form of the camelCase methods.
// `VITE_MOCK=1` selects `mockApi`, which lets the UI be built and run in a browser without Rust.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { LanguagePreference } from "./i18n";
import type { Tone } from "./tone";
import type {
  AgentContext,
  AgentOpen,
  AgentStatus,
  AssetWritten,
  BufferText,
  ChangedSince,
  CreatedDoc,
  CreatedFolder,
  DeleteOutcome,
  Distribution,
  DocumentView,
  FileMeta,
  FsBatch,
  GitSnapshot,
  IndexEntry,
  LaunchMode,
  LinkTarget,
  Move,
  MoveOutcome,
  MovePreview,
  NewComment,
  Comment,
  LooseStatus,
  MoveSuggestion,
  MoveUndone,
  NoteCreated,
  NoteDeleted,
  NoteList,
  NoteMoved,
  OpenedFile,
  PathClass,
  PendingEdits,
  NoteRestored,
  NoteSaved,
  NotesFolder,
  NotesHandoff,
  NotesOpened,
  NotesOverview,
  NotesRequest,
  NoteVersion,
  NoteView,
  ParsedBlocks,
  RecentVault,
  SaveOutcome,
  SearchHit,
  SessionState,
  ShellState,
  SourceDoc,
  StartIn,
  TaggedFile,
  TagOverview,
  TreeNode,
  UndoOutcome,
  VaultInfo,
} from "./types";

/** Payloads of the events emitted by the backend. */
export type EventMap = {
  "fs-change": FsBatch;
  "tree-changed": null;
  "git-status-changed": GitSnapshot;
  "index-progress": { indexed: number; total: number };
  "meta-changed": null;
  "image-refs-changed": null;
  /** The Markdown documents that are unread changed: the payload is all of them, sorted. */
  "unread-changed": string[];
  /** The system asked this window (which shows no project) to open a file or folder (macOS: Finder, the Dock icon). */
  "open-path": string;
  /** The system asked for a file of the project this window shows (macOS: Finder): its path in the project. */
  "show-in-project": string;
  /** The agent connection changed: switched, an agent came or went, or one asked something. */
  "agent-status": AgentStatus;
  /** An agent asked to show a place: `takeAgentOpen` has it. */
  "agent-open": null;
  /** The comments changed: one was added, completed or deleted, or a document moved. `listComments` has them. */
  "comments-changed": null;
  /** Another window or launch asked the Notes window to come forward: `takeNotesRequest` has what it asked for. */
  "notes-request": null;
};
export type EventName = keyof EventMap;
export type Unlisten = () => void;

export interface Api {
  openVault(path: string): Promise<VaultInfo>;
  /** The path the app starts with: the one the system opened it with, else the CLI argument, if any. */
  initialPath(): Promise<string | null>;
  recentVaults(): Promise<RecentVault[]>;
  /** The language the interface starts in: the one picked in Settings, else the one the installer recorded, else "system". */
  appLanguage(): Promise<LanguagePreference>;
  /** Which build this is: the Microsoft Store build leaves out what the Store does for the app (updates). */
  appDistribution(): Promise<Distribution>;
  /** Keeps the language picked in Settings (an app-wide choice, not a project one). */
  setAppLanguage(language: LanguagePreference): Promise<void>;
  /** The tone of the light theme the person picked (an app-wide choice): "white" until they pick one. */
  appTone(): Promise<Tone>;
  /** Keeps the tone of the light theme picked in Settings. */
  setAppTone(tone: Tone): Promise<void>;
  /** What "Open with TrailMark" has registered with the file manager, read from the system each time. */
  shellIntegrationState(): Promise<ShellState>;
  /** Adds or removes the right-click menu entry; `locale` (`en`, `pt-BR`, `es`, `de`, `zh-CN`) is the language it is worded in. */
  setShellContextMenu(enabled: boolean, locale: string): Promise<ShellState>;
  /** Adds or removes TrailMark in the "Open with" list of Markdown files. */
  setShellFileAssociation(enabled: boolean, locale: string): Promise<ShellState>;
  /** Rewords the entries that exist after the interface language changed. */
  refreshShellLabels(locale: string): Promise<void>;
  /** Opens the page of Windows Settings where the default app of a file type is chosen. */
  openDefaultApps(): Promise<void>;
  /** Main root plus worktrees as top-level roots. */
  /** The folder's tree. `showIgnored` also lists what version control ignores; `showHidden` lists `.trailmark/`. */
  listTree(showIgnored: boolean, showHidden: boolean): Promise<TreeNode>;
  readDocument(path: string): Promise<DocumentView>;
  resolveLink(from: string, href: string): Promise<LinkTarget>;
  gitSnapshot(): Promise<GitSnapshot>;
  setGitBaseline(kind: "lastVisit" | "head"): Promise<GitSnapshot>;
  sinceLastVisit(): Promise<ChangedSince>;
  /** Last doc, scroll, zoom, theme, follow. */
  saveSession(s: SessionState): Promise<void>;
  loadSession(): Promise<SessionState | null>;
  /** The absolute path of a file of the folder (for "Copy path"). */
  absolutePath(path: string): Promise<string>;
  /** Opens a document, picture, sound or video of the folder with the system's default application. */
  openExternal(path: string): Promise<void>;
  /**
   * Opens any file of the folder with the system's default application. Rejects with kind "blockedType" (the subject is the
   * kind of file, such as ".ps1") for a program, script, installer, shortcut or macro document: those are never opened.
   */
  openWithDefaultApp(path: string): Promise<void>;
  /**
   * Opens a file of the folder in the person's editor at a line and column (from 1): runs the editor command of the
   * settings, or the system's default application when there is none. Rejects with kind "editor" (and a `reason`:
   * `notFound`, `badArguments`, `spawn`, `template`) when the command can't be run.
   */
  openInEditor(path: string, line?: number, col?: number): Promise<void>;
  /** The editor command, such as `code -g {path}:{line}:{col}`; blank means the system's default application. */
  editorCommand(): Promise<string>;
  /** Keeps the editor command. Rejects (kind "editor", reason "template") when it can't be read: an unknown placeholder, an open quote. */
  setEditorCommand(command: string): Promise<void>;
  /** Shows a file or folder of the open folder in the system's file manager. */
  revealInFolder(path: string): Promise<void>;
  search(query: string, limit?: number): Promise<SearchHit[]>;
  /** The images, sound and video some Markdown document of the folder links to, embeds or defines a reference for (folder paths). */
  referencedImages(): Promise<string[]>;
  /** Creates `.trailmark/` in the project. */
  initProjectFolder(): Promise<void>;
  stateLocation(): Promise<"appData" | "project">;
  /** The tags (front matter and app) and the description of a file. */
  fileMeta(path: string): Promise<FileMeta>;
  /** Replaces the tags the app keeps for a file. Front matter tags are not touched. */
  setFileTags(path: string, tags: string[]): Promise<FileMeta>;
  setFileDescription(path: string, description: string): Promise<FileMeta>;
  /** Every tag with its number of files, the untagged count, missing entries and the state of `meta.toml`. */
  listTags(): Promise<TagOverview>;
  /** Files that have all of `tags`, or (with `untagged`) the files that have none. */
  filesWithTags(tags: string[], untagged: boolean): Promise<TaggedFile[]>;
  /** The tags and description of every file that has some. */
  metaIndex(): Promise<IndexEntry[]>;
  /** Removes the entries kept for files that no longer exist. Resolves to how many. */
  cleanUpMeta(): Promise<number>;
  /** A Markdown file as the editor needs it: the exact text, its hash and its blocks. Rejects for files that can't be edited. */
  documentSource(path: string): Promise<SourceDoc>;
  /** The top-level blocks of some Markdown text (the editor's buffer). */
  parseBlocks(text: string): Promise<ParsedBlocks>;
  /** The reading view's HTML for pieces of Markdown, in order (Visual mode draws its locked blocks with it). An empty string: no HTML, show the source. */
  renderFragments(texts: string[]): Promise<string[]>;
  /** Writes `text` unless the file is no longer the version with `expectedHash`; then it reports a conflict and writes nothing. */
  saveDocument(path: string, text: string, expectedHash: string): Promise<SaveOutcome>;
  /** Creates a Markdown file; rejects with kind "exists" when the name is taken. */
  createDocument(path: string, text: string): Promise<CreatedDoc>;
  /** Writes an image (`data` is base64) into the `assets` folder next to the document. */
  writeAsset(docPath: string, nameHint: string, data: string): Promise<AssetWritten>;
  /**
   * Checks a rename or move (rejects when the destination exists, is outside the project, the name is not usable...)
   * and, with `scanLinks`, lists the links of the documents that would have to change. Nothing is changed yet. The
   * documents in `heldBack` have unsaved edits that cannot be patched (Visual mode): they are left out of the plan
   * and listed as skipped.
   */
  previewMove(moves: Move[], scanLinks: boolean, heldBack: string[]): Promise<MovePreview>;
  /**
   * Makes the move of a preview, once. With `updateLinks` the links it listed are rewritten (a document whose file
   * changed since is skipped and reported); documents in `buffers` have unsaved edits: they are not written, the
   * changes for their text come back instead.
   */
  applyMove(planId: number, updateLinks: boolean, buffers: BufferText[]): Promise<MoveOutcome>;
  /** Sends files and folders to the system's recycle bin. */
  deleteEntries(paths: string[]): Promise<DeleteOutcome>;
  /** Creates a folder; rejects with kind "exists" or "invalidName". */
  newFolder(path: string): Promise<CreatedFolder>;
  /** Reverts an operation (the moves and every rewritten link) as far as the files are unchanged since. */
  undoFileOp(opId: number): Promise<UndoOutcome>;
  /** The Markdown documents that changed since they were last read, sorted (the `unread-changed` event carries the same list). */
  getUnread(): Promise<string[]>;
  /** The person read the document: whatever it holds now counts as read. */
  markRead(path: string): Promise<void>;
  /** The document counts as unread again. */
  markUnread(path: string): Promise<void>;
  /** Every document counts as read. */
  markAllRead(): Promise<void>;
  /** Every document in a folder, and below it, counts as read. */
  markFolderRead(path: string): Promise<void>;
  /**
   * Turns the agent connection of the open project on or off (the setting is saved with the session). A connection that
   * cannot start still resolves: the status says why.
   */
  setAgentConnection(enabled: boolean): Promise<AgentStatus>;
  /** The state of the agent connection (the `agent-status` event carries the same). */
  agentStatus(): Promise<AgentStatus>;
  /** What the window shows, for an agent that asks. Sent when it changes. */
  publishAgentContext(context: AgentContext): Promise<void>;
  /** The place an agent asked to be shown, once; null when there is none. */
  takeAgentOpen(): Promise<AgentOpen | null>;
  /** Writes a comment on a passage of a document. Rejects with kind "emptyComment" or "notADocument". */
  addComment(comment: NewComment): Promise<Comment>;
  /** Every comment of the project, each with its passage found again in the document as it is now (by document and line). */
  listComments(): Promise<Comment[]>;
  /** Checks a comment off (`done`), or opens it again. */
  setCommentDone(id: string, done: boolean): Promise<Comment>;
  deleteComment(id: string): Promise<void>;

  // ---- notes ------------------------------------------------------------------------------------

  /** What this program is: the Notes window (started with `--notes`) or a window for a project. */
  launchMode(): Promise<LaunchMode>;
  /** The notes folder: where it is and whether it is there. */
  notesFolder(): Promise<NotesFolder>;
  /** Points the notes at another folder, one that exists ("Locate..." and Settings). Rejects when it is not a folder. */
  setNotesFolder(path: string): Promise<NotesFolder>;
  /** Makes the notes folder that is not there ("Create it"); nothing else makes it. */
  createNotesFolder(): Promise<NotesFolder>;
  /** Where TrailMark opens when it is started with nothing to open. */
  notesStartIn(): Promise<StartIn>;
  setNotesStartIn(startIn: StartIn): Promise<void>;
  /** The notes folder and what the welcome screen shows of it. Any window can ask; the folder is only read. */
  notesOverview(): Promise<NotesOverview>;
  /** Opens the notes folder as this window's workspace (the Notes window). Rejects when the folder is not there. */
  openNotes(): Promise<NotesOpened>;
  /** The notes, the pinned ones first. */
  listNotes(): Promise<NoteList>;
  /** Makes a new note: an empty file, in the list at once, that stays until the person deletes it. */
  newNote(): Promise<NoteCreated>;
  /**
   * Saves a note's text. `expectedHash` is the hash the editor started from. A note that changed meanwhile is not overwritten:
   * the text is kept as a copy.
   */
  saveNote(path: string, text: string, expectedHash: string): Promise<NoteSaved>;
  /** The note was opened: it is the one to come back to, and its next save keeps an earlier version. */
  noteOpened(path: string): Promise<void>;
  /** No note is open any more. */
  closeNote(): Promise<void>;
  /** Where a note was left: the editor, the caret and the scroll position. */
  setNoteView(path: string, view: NoteView): Promise<void>;
  setNotePinned(path: string, pinned: boolean): Promise<void>;
  /** The order of the pinned notes. */
  setPinOrder(order: string[]): Promise<void>;
  /** Gives a note a real file name; resolves to the name it has now. Rejects with kind "exists" or "invalidName". */
  renameNote(path: string, name: string): Promise<string>;
  /** Sends notes to the recycle bin; `undoDeleteNotes` brings them back while the program runs. */
  deleteNotes(paths: string[]): Promise<NoteDeleted>;
  /** Puts back the notes of a deletion; resolves to the names they have now. */
  undoDeleteNotes(undoId: number): Promise<string[]>;
  /** Sends the notes that were copied and not edited since to the recycle bin. Pinned notes stay. */
  deleteCopiedNotes(): Promise<NoteDeleted>;
  /** The note was copied: it shows as copied until it is edited. */
  markNoteCopied(path: string): Promise<void>;
  /** The earlier versions of a note, the newest first. */
  noteVersions(path: string): Promise<NoteVersion[]>;
  noteVersionText(path: string, id: string): Promise<string>;
  /** Puts an earlier version back; the text the note has now becomes a version first. */
  restoreNoteVersion(path: string, id: string): Promise<NoteRestored>;
  /** Shows the notes folder in the system's file manager. */
  openNotesFolder(): Promise<void>;
  /** Shares a note with agents, or stops sharing it. */
  setNoteShared(path: string, shared: boolean): Promise<void>;
  /** What the Save dialog starts with for a note: the project opened last, and a name made from the title. */
  moveSuggestion(path: string): Promise<MoveSuggestion>;
  /** Moves a note into a project: the text is written to `destination`, the note goes to the recycle bin, the copy is a file of the list. */
  moveNoteToProject(path: string, destination: string): Promise<NoteMoved>;
  /** Undoes a move: the note is back, and the copy goes unless it was changed since. */
  undoMoveNote(undoId: number): Promise<MoveUndone>;
  /** Starts TrailMark on the project that holds a file, showing it. */
  openInProject(path: string): Promise<void>;
  /** The file this program was started to show once its project is open; asked once. */
  launchShow(): Promise<string | null>;
  /** What a path of the disk is, and where it is in the open project (when this window shows one that holds it). */
  classifyPath(path: string): Promise<PathClass>;
  /** Opens a file in the Notes window: a note when it is one of the notes folder, else a loose file under Files. */
  openLooseFile(path: string): Promise<OpenedFile>;
  /** Looks at every loose file: as it was read, changed since, or gone (a rename in its folder is followed). */
  looseStatus(): Promise<LooseStatus[]>;
  /** Takes a loose file out of the list; the file is not touched. */
  removeLooseFile(path: string): Promise<void>;
  /** Points a listed file at another file ("Locate..."). */
  locateLooseFile(path: string, newPath: string): Promise<OpenedFile>;
  /** Keeps the text typed into a loose file, which is never written until the person saves. */
  savePending(path: string, text: string, baseHash: string): Promise<void>;
  /** The text kept for a loose file, if any. */
  loadPending(path: string): Promise<PendingEdits | null>;
  clearPending(path: string): Promise<void>;
  /** Makes a note of the text typed into a loose file, "{name} (my edits)"; resolves to the note's name. */
  keepEditsAsNote(path: string, text: string): Promise<string>;
  /** Brings the Notes window forward, with a new note ready, a note shown or a file of the disk opened. Starts it when it is not running. */
  showNotes(newNote: boolean, open: string | null, openFile?: string | null): Promise<void>;
  /**
   * A window that shows only the welcome screen asks for the Notes window. When none is running this window becomes it
   * ("became": same window, same place; the UI shows the notes in it). When one runs in another program it is asked to come
   * forward with the request ("forwarded"), and this window is to close. Rejects for a window that has a project open.
   */
  becomeNotesWindow(
    newNote: boolean,
    open: string | null,
    openFile?: string | null,
  ): Promise<NotesHandoff>;
  /** What was asked of the Notes window while it was not looking, once; null when nothing. */
  takeNotesRequest(): Promise<NotesRequest | null>;

  /** Subscribes to a backend event. Resolves to the function that unsubscribes. */
  on<E extends EventName>(event: E, handler: (payload: EventMap[E]) => void): Promise<Unlisten>;
}

// ---------------------------------------------------------------------------------------------
// Real implementation: Tauri commands and events
// ---------------------------------------------------------------------------------------------

export const tauriApi: Api = {
  openVault: (path) => invoke<VaultInfo>("open_vault", { path }),
  initialPath: () => invoke<string | null>("initial_path"),
  recentVaults: () => invoke<RecentVault[]>("recent_vaults"),
  appLanguage: () => invoke<LanguagePreference>("app_language"),
  appDistribution: () => invoke<Distribution>("app_distribution"),
  setAppLanguage: (language) => invoke<void>("set_app_language", { language }),
  appTone: () => invoke<Tone>("app_tone"),
  setAppTone: (tone) => invoke<void>("set_app_tone", { tone }),
  shellIntegrationState: () => invoke<ShellState>("shell_integration_state"),
  setShellContextMenu: (enabled, locale) =>
    invoke<ShellState>("set_shell_context_menu", { enabled, locale }),
  setShellFileAssociation: (enabled, locale) =>
    invoke<ShellState>("set_shell_file_association", { enabled, locale }),
  refreshShellLabels: (locale) => invoke<void>("refresh_shell_labels", { locale }),
  openDefaultApps: () => invoke<void>("open_default_apps"),
  listTree: (showIgnored, showHidden) => invoke<TreeNode>("list_tree", { showIgnored, showHidden }),
  readDocument: (path) => invoke<DocumentView>("read_document", { path }),
  resolveLink: (from, href) => invoke<LinkTarget>("resolve_link", { from, href }),
  gitSnapshot: () => invoke<GitSnapshot>("git_snapshot"),
  setGitBaseline: (kind) => invoke<GitSnapshot>("set_git_baseline", { kind }),
  sinceLastVisit: () => invoke<ChangedSince>("since_last_visit"),
  saveSession: (s) => invoke<void>("save_session", { s }),
  loadSession: () => invoke<SessionState | null>("load_session"),
  absolutePath: (path) => invoke<string>("absolute_path", { path }),
  openExternal: (path) => invoke<void>("open_external", { path }),
  openWithDefaultApp: (path) => invoke<void>("open_with_default_app", { path }),
  openInEditor: (path, line, col) => invoke<void>("open_in_editor", { path, line, col }),
  editorCommand: () => invoke<string>("editor_command"),
  setEditorCommand: (command) => invoke<void>("set_editor_command", { command }),
  revealInFolder: (path) => invoke<void>("reveal_in_folder", { path }),
  search: (query, limit) => invoke<SearchHit[]>("search", { query, limit }),
  referencedImages: () => invoke<string[]>("referenced_images"),
  initProjectFolder: () => invoke<void>("init_project_folder"),
  stateLocation: () => invoke<"appData" | "project">("state_location"),
  fileMeta: (path) => invoke<FileMeta>("file_meta", { path }),
  setFileTags: (path, tags) => invoke<FileMeta>("set_file_tags", { path, tags }),
  setFileDescription: (path, description) =>
    invoke<FileMeta>("set_file_description", { path, description }),
  listTags: () => invoke<TagOverview>("list_tags"),
  filesWithTags: (tags, untagged) => invoke<TaggedFile[]>("files_with_tags", { tags, untagged }),
  metaIndex: () => invoke<IndexEntry[]>("meta_index"),
  cleanUpMeta: () => invoke<number>("clean_up_meta"),
  documentSource: (path) => invoke<SourceDoc>("document_source", { path }),
  parseBlocks: (text) => invoke<ParsedBlocks>("parse_blocks", { text }),
  renderFragments: (texts) => invoke<string[]>("render_fragments", { texts }),
  saveDocument: (path, text, expectedHash) =>
    invoke<SaveOutcome>("save_document", { path, text, expectedHash }),
  createDocument: (path, text) => invoke<CreatedDoc>("create_document", { path, text }),
  writeAsset: (docPath, nameHint, data) =>
    invoke<AssetWritten>("write_asset", { docPath, nameHint, data }),
  previewMove: (moves, scanLinks, heldBack) =>
    invoke<MovePreview>("preview_move", { moves, scanLinks, heldBack }),
  applyMove: (planId, updateLinks, buffers) =>
    invoke<MoveOutcome>("apply_move", { planId, updateLinks, buffers }),
  deleteEntries: (paths) => invoke<DeleteOutcome>("delete_entries", { paths }),
  newFolder: (path) => invoke<CreatedFolder>("new_folder", { path }),
  undoFileOp: (opId) => invoke<UndoOutcome>("undo_file_op", { opId }),
  getUnread: () => invoke<string[]>("get_unread"),
  markRead: (path) => invoke<void>("mark_read", { path }),
  markUnread: (path) => invoke<void>("mark_unread", { path }),
  markAllRead: () => invoke<void>("mark_all_read"),
  markFolderRead: (path) => invoke<void>("mark_folder_read", { path }),
  setAgentConnection: (enabled) => invoke<AgentStatus>("set_agent_connection", { enabled }),
  agentStatus: () => invoke<AgentStatus>("agent_status"),
  publishAgentContext: (context) => invoke<void>("publish_agent_context", { context }),
  takeAgentOpen: () => invoke<AgentOpen | null>("take_agent_open"),
  addComment: (comment) => invoke<Comment>("add_comment", { comment }),
  listComments: () => invoke<Comment[]>("list_comments"),
  setCommentDone: (id, done) => invoke<Comment>("set_comment_done", { id, done }),
  deleteComment: (id) => invoke<void>("delete_comment", { id }),
  launchMode: () => invoke<LaunchMode>("launch_mode"),
  notesFolder: () => invoke<NotesFolder>("notes_folder"),
  setNotesFolder: (path) => invoke<NotesFolder>("set_notes_folder", { path }),
  createNotesFolder: () => invoke<NotesFolder>("create_notes_folder"),
  notesStartIn: () => invoke<StartIn>("notes_start_in"),
  setNotesStartIn: (startIn) => invoke<void>("set_notes_start_in", { startIn }),
  notesOverview: () => invoke<NotesOverview>("notes_overview"),
  openNotes: () => invoke<NotesOpened>("open_notes"),
  listNotes: () => invoke<NoteList>("list_notes"),
  newNote: () => invoke<NoteCreated>("new_note"),
  saveNote: (path, text, expectedHash) =>
    invoke<NoteSaved>("save_note", { path, text, expectedHash }),
  noteOpened: (path) => invoke<void>("note_opened", { path }),
  closeNote: () => invoke<void>("close_note"),
  setNoteView: (path, view) => invoke<void>("set_note_view", { path, view }),
  setNotePinned: (path, pinned) => invoke<void>("set_note_pinned", { path, pinned }),
  setPinOrder: (order) => invoke<void>("set_pin_order", { order }),
  renameNote: (path, name) => invoke<string>("rename_note", { path, name }),
  deleteNotes: (paths) => invoke<NoteDeleted>("delete_notes", { paths }),
  undoDeleteNotes: (undoId) => invoke<string[]>("undo_delete_notes", { undoId }),
  deleteCopiedNotes: () => invoke<NoteDeleted>("delete_copied_notes"),
  markNoteCopied: (path) => invoke<void>("mark_note_copied", { path }),
  noteVersions: (path) => invoke<NoteVersion[]>("note_versions", { path }),
  noteVersionText: (path, id) => invoke<string>("note_version_text", { path, id }),
  restoreNoteVersion: (path, id) => invoke<NoteRestored>("restore_note_version", { path, id }),
  openNotesFolder: () => invoke<void>("open_notes_folder"),
  setNoteShared: (path, shared) => invoke<void>("set_note_shared", { path, shared }),
  moveSuggestion: (path) => invoke<MoveSuggestion>("move_suggestion", { path }),
  moveNoteToProject: (path, destination) =>
    invoke<NoteMoved>("move_note_to_project", { path, destination }),
  undoMoveNote: (undoId) => invoke<MoveUndone>("undo_move_note", { undoId }),
  openInProject: (path) => invoke<void>("open_in_project", { path }),
  launchShow: () => invoke<string | null>("launch_show"),
  classifyPath: (path) => invoke<PathClass>("classify_path", { path }),
  openLooseFile: (path) => invoke<OpenedFile>("open_loose_file", { path }),
  looseStatus: () => invoke<LooseStatus[]>("loose_status"),
  removeLooseFile: (path) => invoke<void>("remove_loose_file", { path }),
  locateLooseFile: (path, newPath) => invoke<OpenedFile>("locate_loose_file", { path, newPath }),
  savePending: (path, text, baseHash) => invoke<void>("save_pending", { path, text, baseHash }),
  loadPending: (path) => invoke<PendingEdits | null>("load_pending", { path }),
  clearPending: (path) => invoke<void>("clear_pending", { path }),
  keepEditsAsNote: (path, text) => invoke<string>("keep_edits_as_note", { path, text }),
  showNotes: (newNote, open, openFile = null) =>
    invoke<void>("show_notes", { newNote, open, openFile }),
  becomeNotesWindow: (newNote, open, openFile = null) =>
    invoke<NotesHandoff>("become_notes_window", { newNote, open, openFile }),
  takeNotesRequest: () => invoke<NotesRequest | null>("take_notes_request"),
  on: (event, handler) => listen(event, (e) => handler(e.payload as never)),
};

// ---------------------------------------------------------------------------------------------
// Mock implementation: a realistic fake vault, no backend needed. The implementation (./mock/mockApi.ts)
// and its fixtures are loaded on first use, and dropped from production builds.
// ---------------------------------------------------------------------------------------------

type AnyHandler = (payload: never) => void;
const mockListeners = new Map<EventName, Set<AnyHandler>>();

/** Lets UI code, the mock driver and tests simulate a backend event while running against `mockApi`. */
export function emitMock<E extends EventName>(event: E, payload: EventMap[E]): void {
  for (const handler of mockListeners.get(event) ?? []) {
    (handler as (p: EventMap[E]) => void)(payload);
  }
}

/** Used by the mock implementation to subscribe a handler; returns the unsubscribe function. */
export function addMockListener<E extends EventName>(
  event: E,
  handler: (payload: EventMap[E]) => void,
): Unlisten {
  const handlers = mockListeners.get(event) ?? new Set<AnyHandler>();
  handlers.add(handler as AnyHandler);
  mockListeners.set(event, handlers);
  return () => {
    handlers.delete(handler as AnyHandler);
  };
}

type CommandName = Exclude<keyof Api, "on">;

function createMockApi(): Api {
  const load = (): Promise<Api> => import("./mock/mockApi").then((m) => m.mockImpl);
  const command = <K extends CommandName>(name: K): Api[K] =>
    ((...args: unknown[]) =>
      load().then((impl) => (impl[name] as (...a: unknown[]) => unknown)(...args))) as Api[K];
  return {
    openVault: command("openVault"),
    initialPath: command("initialPath"),
    recentVaults: command("recentVaults"),
    appLanguage: command("appLanguage"),
    appDistribution: command("appDistribution"),
    setAppLanguage: command("setAppLanguage"),
    appTone: command("appTone"),
    setAppTone: command("setAppTone"),
    shellIntegrationState: command("shellIntegrationState"),
    setShellContextMenu: command("setShellContextMenu"),
    setShellFileAssociation: command("setShellFileAssociation"),
    refreshShellLabels: command("refreshShellLabels"),
    openDefaultApps: command("openDefaultApps"),
    listTree: command("listTree"),
    readDocument: command("readDocument"),
    resolveLink: command("resolveLink"),
    gitSnapshot: command("gitSnapshot"),
    setGitBaseline: command("setGitBaseline"),
    sinceLastVisit: command("sinceLastVisit"),
    saveSession: command("saveSession"),
    loadSession: command("loadSession"),
    absolutePath: command("absolutePath"),
    openExternal: command("openExternal"),
    openWithDefaultApp: command("openWithDefaultApp"),
    openInEditor: command("openInEditor"),
    editorCommand: command("editorCommand"),
    setEditorCommand: command("setEditorCommand"),
    revealInFolder: command("revealInFolder"),
    search: command("search"),
    referencedImages: command("referencedImages"),
    initProjectFolder: command("initProjectFolder"),
    stateLocation: command("stateLocation"),
    fileMeta: command("fileMeta"),
    setFileTags: command("setFileTags"),
    setFileDescription: command("setFileDescription"),
    listTags: command("listTags"),
    filesWithTags: command("filesWithTags"),
    metaIndex: command("metaIndex"),
    cleanUpMeta: command("cleanUpMeta"),
    documentSource: command("documentSource"),
    parseBlocks: command("parseBlocks"),
    renderFragments: command("renderFragments"),
    saveDocument: command("saveDocument"),
    createDocument: command("createDocument"),
    writeAsset: command("writeAsset"),
    previewMove: command("previewMove"),
    applyMove: command("applyMove"),
    deleteEntries: command("deleteEntries"),
    newFolder: command("newFolder"),
    undoFileOp: command("undoFileOp"),
    getUnread: command("getUnread"),
    markRead: command("markRead"),
    markUnread: command("markUnread"),
    markAllRead: command("markAllRead"),
    markFolderRead: command("markFolderRead"),
    setAgentConnection: command("setAgentConnection"),
    agentStatus: command("agentStatus"),
    publishAgentContext: command("publishAgentContext"),
    takeAgentOpen: command("takeAgentOpen"),
    addComment: command("addComment"),
    listComments: command("listComments"),
    setCommentDone: command("setCommentDone"),
    deleteComment: command("deleteComment"),
    launchMode: command("launchMode"),
    notesFolder: command("notesFolder"),
    setNotesFolder: command("setNotesFolder"),
    createNotesFolder: command("createNotesFolder"),
    notesStartIn: command("notesStartIn"),
    setNotesStartIn: command("setNotesStartIn"),
    notesOverview: command("notesOverview"),
    openNotes: command("openNotes"),
    listNotes: command("listNotes"),
    newNote: command("newNote"),
    saveNote: command("saveNote"),
    noteOpened: command("noteOpened"),
    closeNote: command("closeNote"),
    setNoteView: command("setNoteView"),
    setNotePinned: command("setNotePinned"),
    setPinOrder: command("setPinOrder"),
    renameNote: command("renameNote"),
    deleteNotes: command("deleteNotes"),
    undoDeleteNotes: command("undoDeleteNotes"),
    deleteCopiedNotes: command("deleteCopiedNotes"),
    markNoteCopied: command("markNoteCopied"),
    noteVersions: command("noteVersions"),
    noteVersionText: command("noteVersionText"),
    restoreNoteVersion: command("restoreNoteVersion"),
    openNotesFolder: command("openNotesFolder"),
    setNoteShared: command("setNoteShared"),
    moveSuggestion: command("moveSuggestion"),
    moveNoteToProject: command("moveNoteToProject"),
    undoMoveNote: command("undoMoveNote"),
    openInProject: command("openInProject"),
    launchShow: command("launchShow"),
    classifyPath: command("classifyPath"),
    openLooseFile: command("openLooseFile"),
    looseStatus: command("looseStatus"),
    removeLooseFile: command("removeLooseFile"),
    locateLooseFile: command("locateLooseFile"),
    savePending: command("savePending"),
    loadPending: command("loadPending"),
    clearPending: command("clearPending"),
    keepEditsAsNote: command("keepEditsAsNote"),
    showNotes: command("showNotes"),
    becomeNotesWindow: command("becomeNotesWindow"),
    takeNotesRequest: command("takeNotesRequest"),
    on: async (event, handler) => addMockListener(event, handler),
  };
}

export const mockApi: Api = /* @__PURE__ */ createMockApi();

/** The API the app uses: the mock when built with `VITE_MOCK=1`, the Tauri backend otherwise. */
export const api: Api = import.meta.env.VITE_MOCK === "1" ? mockApi : tauriApi;
export default api;
