// The ONLY module that talks to the backend. Everything else in the UI imports `api` from here.
// Command names are the snake_case form of the camelCase methods.
// `VITE_MOCK=1` selects `mockApi`, which lets the UI be built and run in a browser without Rust.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { LanguagePreference } from "./i18n";
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
  DocumentView,
  FileMeta,
  FsBatch,
  GitSnapshot,
  IndexEntry,
  LinkTarget,
  Move,
  MoveOutcome,
  MovePreview,
  ParsedBlocks,
  RecentVault,
  SaveOutcome,
  SearchHit,
  SessionState,
  ShellState,
  SourceDoc,
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
  /** The agent connection changed: switched, an agent came or went, or one asked something. */
  "agent-status": AgentStatus;
  /** An agent asked to show a place: `takeAgentOpen` has it. */
  "agent-open": null;
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
  /** Keeps the language picked in Settings (an app-wide choice, not a project one). */
  setAppLanguage(language: LanguagePreference): Promise<void>;
  /** What "Open with TrailMark" has registered with the file manager, read from the system each time. */
  shellIntegrationState(): Promise<ShellState>;
  /** Adds or removes the right-click menu entry; `locale` (`en`, `pt-BR`, `es`, `de`) is the language it is worded in. */
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
  /** Opens a document or image of the folder with the system's default application. */
  openExternal(path: string): Promise<void>;
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
  /** The images some Markdown document of the folder links to, embeds or defines a reference for (folder paths). */
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
  setAppLanguage: (language) => invoke<void>("set_app_language", { language }),
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
    setAppLanguage: command("setAppLanguage"),
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
    on: async (event, handler) => addMockListener(event, handler),
  };
}

export const mockApi: Api = /* @__PURE__ */ createMockApi();

/** The API the app uses: the mock when built with `VITE_MOCK=1`, the Tauri backend otherwise. */
export const api: Api = import.meta.env.VITE_MOCK === "1" ? mockApi : tauriApi;
export default api;
