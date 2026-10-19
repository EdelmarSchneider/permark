// Backend interface. Implementations: `tauri.ts` (real IPC), `mock.ts` (dev:mock + tests). Only `api/` calls Tauri.

import type { AppInfo } from "./generated/AppInfo";
import type { AppSettings } from "./generated/AppSettings";
import type { BootInfo } from "./generated/BootInfo";
import type { CommandName } from "./generated/CommandName";
import type { ContextChanged } from "./generated/ContextChanged";
import type { CloseRequested } from "./generated/CloseRequested";
import type { DeleteOutcome } from "./generated/DeleteOutcome";
import type { Body } from "./generated/Body";
import type { ConfigEdit } from "./generated/ConfigEdit";
import type { ConfigEdited } from "./generated/ConfigEdited";
import type { EditSource } from "./generated/EditSource";
import type { IgnoreMatches } from "./generated/IgnoreMatches";
import type { NewValue } from "./generated/NewValue";
import type { Row } from "./generated/Row";
import type { RowValue } from "./generated/RowValue";
import type { Scalar } from "./generated/Scalar";
import type { ScalarKind } from "./generated/ScalarKind";
import type { SecretRange } from "./generated/SecretRange";
import type { Sheet } from "./generated/Sheet";
import type { Group } from "./generated/Group";
import type { LineEdit } from "./generated/LineEdit";
import type { Measure } from "./generated/Measure";
import type { SaveOutcome } from "./generated/SaveOutcome";
import type { Segment } from "./generated/Segment";
import type { TextKind } from "./generated/TextKind";
import type { DocumentChanged } from "./generated/DocumentChanged";
import type { DocumentView } from "./generated/DocumentView";
import type { IndexProgress } from "./generated/IndexProgress";
import type { IndexStatus } from "./generated/IndexStatus";
import type { IpcError } from "./generated/IpcError";
import type { LinkTarget } from "./generated/LinkTarget";
import type { MoveOutcome } from "./generated/MoveOutcome";
import type { MovePlan } from "./generated/MovePlan";
import type { MoveSpec } from "./generated/MoveSpec";
import type { NewKind } from "./generated/NewKind";
import type { OpenFailed } from "./generated/OpenFailed";
import type { OutlineItem } from "./generated/OutlineItem";
import type { PathResults } from "./generated/PathResults";
import type { Recent } from "./generated/Recent";
import type { SearchOptions } from "./generated/SearchOptions";
import type { SectionResults } from "./generated/SectionResults";
import type { OpenMode } from "./generated/OpenMode";
import type { OpenOutcome } from "./generated/OpenOutcome";
import type { OpenPlacement } from "./generated/OpenPlacement";
import type { RecentFolder } from "./generated/RecentFolder";
import type { SettingChange } from "./generated/SettingChange";
import type { SettingsChanged } from "./generated/SettingsChanged";
import type { StateRecovered } from "./generated/StateRecovered";
import type { TreeChanged } from "./generated/TreeChanged";
import type { TreeFilter } from "./generated/TreeFilter";
import type { TreeSnapshot } from "./generated/TreeSnapshot";
import type { UndoOutcome } from "./generated/UndoOutcome";
import type { WindowChanged } from "./generated/WindowChanged";
import type { WorkspaceId } from "./generated/WorkspaceId";
import type { WorkspacesChanged } from "./generated/WorkspacesChanged";
import type { WorkspaceSummary } from "./generated/WorkspaceSummary";
import type { Basis } from "./generated/Basis";
import type { BaselineKind } from "./generated/BaselineKind";
import type { FeedItem } from "./generated/FeedItem";
import type { GitFile } from "./generated/GitFile";
import type { GitSnapshot } from "./generated/GitSnapshot";
import type { Letter } from "./generated/Letter";
import type { ReviewChange } from "./generated/ReviewChange";
import type { ReviewChangeKind } from "./generated/ReviewChangeKind";
import type { ReviewStatus } from "./generated/ReviewStatus";
import type { ReviewView } from "./generated/ReviewView";
import type { SinceVisit } from "./generated/SinceVisit";
import type { UnreadList } from "./generated/UnreadList";
import type { AgentMode } from "./generated/AgentMode";
import type { AgentRequest } from "./generated/AgentRequest";
import type { AgentSection } from "./generated/AgentSection";
import type { AgentSelection } from "./generated/AgentSelection";
import type { AgentServerState } from "./generated/AgentServerState";
import type { AgentStatus } from "./generated/AgentStatus";
import type { AgentWindowContext } from "./generated/AgentWindowContext";
import type { BrokenDoc } from "./generated/BrokenDoc";
import type { BrokenLink } from "./generated/BrokenLink";
import type { BrokenReport } from "./generated/BrokenReport";
import type { BrokenSummary } from "./generated/BrokenSummary";
import type { By } from "./generated/By";
import type { CommentScope } from "./generated/CommentScope";
import type { CommentView } from "./generated/CommentView";
import type { FileMeta } from "./generated/FileMeta";
import type { IndexEntry } from "./generated/IndexEntry";
import type { Interest } from "./generated/Interest";
import type { MetaProblem } from "./generated/MetaProblem";
import type { PassageState } from "./generated/PassageState";
import type { QuietMarks } from "./generated/QuietMarks";
import type { ReplyView } from "./generated/ReplyView";
import type { TagCountView } from "./generated/TagCountView";
import type { TagRefView } from "./generated/TagRefView";
import type { TaggedFile } from "./generated/TaggedFile";
import type { TagStyleView } from "./generated/TagStyleView";
import type { TagsIndex } from "./generated/TagsIndex";
import type { TagsLocation } from "./generated/TagsLocation";
import type { TagsOverview } from "./generated/TagsOverview";
import type { TagsStatus } from "./generated/TagsStatus";
import type { Deleted } from "./generated/Deleted";
import type { EntryKind } from "./generated/EntryKind";
import type { FolderInfo } from "./generated/FolderInfo";
import type { FolderStatus } from "./generated/FolderStatus";
import type { LooseState } from "./generated/LooseState";
import type { MoveUndone } from "./generated/MoveUndone";
import type { Moved } from "./generated/Moved";
import type { NoteEntry } from "./generated/NoteEntry";
import type { NoteList } from "./generated/NoteList";
import type { NotesSummary } from "./generated/NotesSummary";
import type { Version } from "./generated/Version";

export type {
  AppInfo,
  CloseRequested,
  ConfigEdit,
  ConfigEdited,
  EditSource,
  Group,
  IgnoreMatches,
  NewValue,
  Row,
  RowValue,
  Scalar,
  ScalarKind,
  SecretRange,
  Sheet,
  LineEdit,
  Measure,
  SaveOutcome,
  Segment,
  TextKind,
  CommandName,
  AppSettings,
  BootInfo,
  DeleteOutcome,
  DocumentChanged,
  DocumentView,
  IpcError,
  LinkTarget,
  MoveOutcome,
  MovePlan,
  MoveSpec,
  NewKind,
  OpenMode,
  OpenOutcome,
  OpenPlacement,
  RecentFolder,
  SettingChange,
  TreeChanged,
  TreeFilter,
  TreeSnapshot,
  UndoOutcome,
  WorkspaceId,
  WorkspaceSummary,
};
export type { Added } from "./generated/Added";
export type { GitBackendPref } from "./generated/GitBackendPref";
export type { FileHit } from "./generated/FileHit";
export type { FolderHit } from "./generated/FolderHit";
export type { IndexPhase } from "./generated/IndexPhase";
export type { SectionHit } from "./generated/SectionHit";
export type { SnippetPart } from "./generated/SnippetPart";
export type { IndexProgress, IndexStatus, PathResults, Recent, SearchOptions, SectionResults };
export type { Block } from "./generated/Block";
export type { Body } from "./generated/Body";
export type { DocChange } from "./generated/DocChange";
export type { Editability } from "./generated/Editability";
export type { Field } from "./generated/Field";
export type { FieldValue } from "./generated/FieldValue";
export type { FrontMatter } from "./generated/FrontMatter";
export type { MediaKind } from "./generated/MediaKind";
export type { Notice } from "./generated/Notice";
export type { OutlineItem } from "./generated/OutlineItem";
export type { LockReason } from "./generated/LockReason";
export type { SegmentKind } from "./generated/SegmentKind";
export type { MeasureUnit } from "./generated/MeasureUnit";
export type { EditModePref } from "./generated/EditModePref";
export type { Category } from "./generated/Category";
export type { FilterShow } from "./generated/FilterShow";
export type { LinkUpdate } from "./generated/LinkUpdate";
export type { NodeKind } from "./generated/NodeKind";
export type { PlanChange } from "./generated/PlanChange";
export type { PlanFile } from "./generated/PlanFile";
export type { Renamed } from "./generated/Renamed";
export type { RootNode } from "./generated/RootNode";
export type { Skip } from "./generated/Skip";
export type { SkipReason } from "./generated/SkipReason";
export type { TreeDelta } from "./generated/TreeDelta";
export type { TreeNode } from "./generated/TreeNode";
export type { Unlisted } from "./generated/Unlisted";
export type { UndoKind } from "./generated/UndoKind";

/** JSON value as the backend stores it (project session values). */
export type JsonValue =
  null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

/** IPC commands: name -> args + result. Mirror of `src-tauri/src/commands.rs`; DTOs from `generated/` (ts-rs). */
export interface Commands {
  app_info: { args: Record<string, never>; result: AppInfo };
  window_boot: { args: Record<string, never>; result: BootInfo };
  open_path: { args: { path: string; placement: OpenPlacement }; result: OpenOutcome };
  open_notes: { args: { placement: OpenPlacement }; result: OpenOutcome };
  open_in_project: { args: { path: string }; result: null };
  show_welcome: { args: Record<string, never>; result: null };
  list_workspaces: { args: Record<string, never>; result: WorkspaceSummary[] };
  focus_workspace: { args: { id: WorkspaceId }; result: null };
  recent_list: { args: Record<string, never>; result: RecentFolder[] };
  recent_remove: { args: { path: string }; result: null };
  settings_get: { args: Record<string, never>; result: AppSettings };
  settings_set: { args: { change: SettingChange }; result: AppSettings };
  project_session_get: { args: { key: string }; result: JsonValue | null };
  project_session_set: { args: { key: string; value: JsonValue | null }; result: null };
  tree_open: { args: { filter: TreeFilter | null }; result: TreeSnapshot };
  tree_set_filter: { args: { filter: TreeFilter }; result: TreeSnapshot };
  tree_open_folder: { args: { path: string }; result: null };
  tree_keep: { args: { path: string | null }; result: null };
  tree_close: { args: Record<string, never>; result: null };
  fileops_plan_move: { args: { moves: MoveSpec[]; updateLinks: boolean }; result: MovePlan };
  fileops_apply_move: { args: { planId: number; updateLinks: boolean }; result: MoveOutcome };
  fileops_delete: { args: { paths: string[] }; result: DeleteOutcome };
  fileops_new: { args: { dir: string; name: string; kind: NewKind }; result: string };
  fileops_undo: { args: Record<string, never>; result: UndoOutcome | null };
  file_abs_path: { args: { path: string }; result: string };
  file_open_external: { args: { path: string; mode: OpenMode }; result: null };
  show_document: { args: { path: string | null }; result: null };
  document_open: { args: { path: string; known: string[] }; result: DocumentView };
  document_close: { args: Record<string, never>; result: null };
  document_text: { args: { path: string }; result: string };
  link_resolve: { args: { doc: string; hrefs: string[] }; result: LinkTarget[] };
  code_refs_resolve: { args: { doc: string; written: string[] }; result: string[][] };
  open_in_editor: {
    args: { path: string; line: number | null; col: number | null };
    result: null;
  };
  open_web: { args: { url: string }; result: null };
  edit_source: { args: { path: string }; result: EditSource };
  edit_segments: { args: { text: string }; result: Segment[] };
  edit_save: {
    args: { path: string; seenHash: string; edits: LineEdit[] };
    result: SaveOutcome;
  };
  edit_save_as: { args: { path: string; text: string; exact: boolean }; result: string };
  /** Raw body (the picture's bytes); `doc` + `ext` travel as headers. */
  edit_write_asset: { args: { doc: string; ext: string; bytes: Uint8Array }; result: string };
  edit_set_unsaved: { args: { path: string | null }; result: null };
  edit_headings: { args: { path: string }; result: OutlineItem[] };
  edit_link_targets: { args: Record<string, never>; result: string[] };
  config_sheet: { args: { path: string; text: string }; result: Body };
  config_edit: { args: { path: string; text: string; edit: ConfigEdit }; result: ConfigEdited };
  config_secrets: { args: { path: string; text: string }; result: SecretRange[] };
  ignore_matches: { args: { path: string; text: string | null }; result: IgnoreMatches };
  analyze: {
    args: { path: string | null; text: string | null; kind: TextKind };
    result: Measure[];
  };
  search_status: { args: Record<string, never>; result: IndexStatus };
  search_sections: {
    args: { query: string; limit: number; options: SearchOptions };
    result: SectionResults;
  };
  search_files: {
    args: { query: string; limit: number; folderLimit: number; options: SearchOptions };
    result: PathResults;
  };
  search_recent: {
    args: { opened: string[]; limit: number; options: SearchOptions };
    result: Recent;
  };
  plugin_call: { args: { plugin: string; method: string; args: JsonValue }; result: JsonValue };
  window_close: { args: Record<string, never>; result: null };
}

/**
 * What running plugins serve (`plugin_call`): `<plugin>.<method>` -> args + result. Mirror of the plugins' `rpc`
 * registrations; DTOs from `generated/`. A plugin that is off answers `unavailable`.
 */
export interface PluginMethods {
  "git.snapshot": { args: Record<string, never>; result: GitSnapshot | null };
  "git.refresh": { args: Record<string, never>; result: null };
  "git.setBaseline": { args: { kind: BaselineKind }; result: null };
  "live-feed.items": { args: Record<string, never>; result: FeedItem[] };
  "live-feed.clear": { args: Record<string, never>; result: null };
  "unread.list": { args: Record<string, never>; result: UnreadList };
  "unread.sinceLastVisit": { args: Record<string, never>; result: SinceVisit };
  "unread.markRead": { args: { path: string; hash: string }; result: null };
  "unread.markUnread": { args: { path: string }; result: null };
  "unread.markFolderRead": { args: { folder: string }; result: null };
  "review.status": { args: { path: string }; result: ReviewStatus };
  "review.open": { args: { path: string; basis?: Basis }; result: ReviewView };
  "review.keep": {
    args: { path: string; hash: string; index: number; basis?: Basis };
    result: ReviewView;
  };
  "review.keepAll": { args: { path: string; hash: string; basis?: Basis }; result: ReviewView };
  "review.revert": {
    args: { path: string; hash: string; index: number; basis?: Basis };
    result: ReviewView;
  };
  "review.revertAll": { args: { path: string; hash: string; basis?: Basis }; result: ReviewView };
  "review.undo": { args: { path: string; basis?: Basis }; result: ReviewView };
  "review.release": { args: { path: string }; result: null };
  "review.restore": { args: { path: string }; result: null };
  "review.setBasis": { args: { basis: Basis }; result: null };
  "review.basis": { args: Record<string, never>; result: Basis };
  "tags.index": { args: Record<string, never>; result: TagsIndex };
  "tags.overview": { args: Record<string, never>; result: TagsOverview };
  "tags.status": { args: Record<string, never>; result: TagsStatus };
  "tags.filesWith": { args: { tags: string[]; untagged?: boolean }; result: TaggedFile[] };
  "tags.fileMeta": { args: { path: string }; result: FileMeta };
  "tags.setTags": { args: { path: string; tags: string[] }; result: FileMeta };
  "tags.setDescription": { args: { path: string; description: string }; result: FileMeta };
  "tags.setStyle": {
    args: { tag: string; color?: string | null; icon?: string | null };
    result: null;
  };
  "tags.orphans": { args: Record<string, never>; result: string[] };
  "tags.cleanUp": { args: Record<string, never>; result: number };
  "comments.list": {
    args: { path?: string | null; scope?: CommentScope };
    result: CommentView[];
  };
  "comments.add": {
    args: {
      path: string;
      quote: string;
      text: string;
      headingPath: string[];
      line: number;
      endLine: number;
    };
    result: CommentView;
  };
  "comments.setDone": { args: { id: string; done: boolean }; result: null };
  "comments.remove": { args: { id: string }; result: null };
  "comments.reply": { args: { id: string; text: string }; result: null };
  "quiet.rules": { args: Record<string, never>; result: QuietMarks };
  "quiet.set": { args: { paths: string[]; interest: Interest }; result: QuietMarks };
  "broken-links.list": { args: Record<string, never>; result: BrokenReport };
  "notes.folder": { args: Record<string, never>; result: FolderInfo };
  "notes.createFolder": { args: Record<string, never>; result: FolderInfo };
  "notes.list": { args: Record<string, never>; result: NoteList };
  "notes.summary": { args: Record<string, never>; result: NotesSummary };
  "notes.create": { args: Record<string, never>; result: string };
  "notes.delete": { args: { paths: string[] }; result: Deleted };
  "notes.undoDelete": { args: { undoId: number }; result: string[] };
  "notes.rename": { args: { path: string; name: string }; result: string };
  "notes.setPinned": { args: { path: string; pinned: boolean }; result: null };
  "notes.setPinOrder": { args: { order: string[] }; result: null };
  "notes.setShared": { args: { path: string; shared: boolean }; result: null };
  "notes.markCopied": { args: { path: string }; result: null };
  "notes.deleteCopied": { args: Record<string, never>; result: Deleted };
  "notes.opened": { args: { path: string | null }; result: null };
  "notes.versions": { args: { path: string }; result: Version[] };
  "notes.versionText": { args: { path: string; id: string }; result: string };
  "notes.restoreVersion": { args: { path: string; id: string }; result: null };
  "notes.buffer": {
    args: { path: string; baseHash: string; text: string | null };
    result: null;
  };
  "notes.recoveredText": { args: { path: string }; result: string | null };
  "notes.keepRecovered": { args: { path: string }; result: null };
  "notes.discardRecovered": { args: { path: string }; result: null };
  "notes.openFile": { args: { path: string }; result: string };
  "notes.removeFile": { args: { path: string }; result: null };
  "notes.locateFile": { args: { path: string; newPath: string }; result: string };
  "notes.reloadFile": { args: { path: string }; result: null };
  "notes.keepMine": { args: { path: string }; result: null };
  "notes.saveAsNote": { args: { path: string }; result: string };
  "notes.suggestName": { args: { path: string; folder: string | null }; result: string };
  "notes.moveToProject": {
    args: { path: string; folder: string; name: string };
    result: Moved;
  };
  "notes.undoMove": { args: { undoId: number }; result: MoveUndone };
  "agent.status": { args: Record<string, never>; result: AgentStatus };
  "agent.setEnabled": { args: { enabled: boolean }; result: AgentStatus };
  "agent.setGroup": { args: { group: string; on: boolean }; result: AgentStatus };
  "agent.setDefaults": { args: Record<string, never>; result: null };
  "agent.publish": { args: { context: AgentWindowContext }; result: null };
  "agent.setup": { args: Record<string, never>; result: string | null };
}
export type PluginMethod = keyof PluginMethods;

/** Events of project plugins: `{ root, data }` (the folder they are about; a window keeps its own). */
export interface PluginEvents {
  "git:snapshot": GitSnapshot | null;
  "live-feed:items": { upsert: FeedItem[] };
  "unread:changed": UnreadList;
  "tags:changed": null;
  "comments:changed": null;
  "quiet:changed": QuietMarks;
  "broken-links:changed": BrokenSummary;
  "agent:status": AgentStatus;
  "agent:open": { path: string; line: number | null; heading: string | null };
  "agent:ask": { group: string };
}
export type PluginEventName = keyof PluginEvents;
export type PluginEnvelope<E extends PluginEventName> = { root: string; data: PluginEvents[E] };

export type {
  Basis,
  BaselineKind,
  FeedItem,
  GitFile,
  GitSnapshot,
  Letter,
  ReviewChange,
  ReviewChangeKind,
  ReviewStatus,
  ReviewView,
  SinceVisit,
};
export type { UnreadList };
export type {
  AgentMode,
  AgentRequest,
  AgentSection,
  AgentSelection,
  AgentServerState,
  AgentStatus,
  AgentWindowContext,
};
export type {
  Deleted,
  EntryKind,
  FolderInfo,
  FolderStatus,
  LooseState,
  MoveUndone,
  Moved,
  NoteEntry,
  NoteList,
  NotesSummary,
  Version,
};
export type {
  BrokenDoc,
  BrokenLink,
  BrokenReport,
  BrokenSummary,
  By,
  CommentScope,
  CommentView,
  FileMeta,
  IndexEntry,
  Interest,
  MetaProblem,
  PassageState,
  QuietMarks,
  ReplyView,
  TagCountView,
  TagRefView,
  TaggedFile,
  TagStyleView,
  TagsIndex,
  TagsLocation,
  TagsOverview,
  TagsStatus,
};

/** Contract: `Commands` has exactly the registered commands (generated from Rust). Fails to compile on drift. */
type Exact<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;
export const COMMANDS_MATCH_BACKEND: Exact<keyof Commands, CommandName> = true;

/** Backend events: name -> payload. */
export interface Events {
  "window-changed": WindowChanged;
  "context-changed": ContextChanged;
  "workspaces-changed": WorkspacesChanged;
  "settings-changed": SettingsChanged;
  "state-recovered": StateRecovered;
  "open-failed": OpenFailed;
  "tree-changed": TreeChanged;
  "document-changed": DocumentChanged;
  "close-requested": CloseRequested;
  "index-progress": IndexProgress;
}
export type EventName = keyof Events;

export type ApiError =
  | { kind: "backend"; command: CommandName; error: IpcError }
  | { kind: "command"; command: CommandName; message: string }
  | { kind: "window"; message: string };

export type Result<T> = { ok: true; value: T } | { ok: false; error: ApiError };

export type Unlisten = () => void;

/** Files | folders dragged onto the window. */
export type DropEvent =
  { type: "over"; count: number } | { type: "leave" } | { type: "drop"; paths: string[] };

export interface Api {
  appInfo(): Promise<Result<AppInfo>>;
  windowBoot(): Promise<Result<BootInfo>>;
  openPath(path: string, placement: OpenPlacement): Promise<Result<OpenOutcome>>;
  /** Shows the Notes in this window (or a new one, or asks first). */
  openNotes(placement: OpenPlacement): Promise<Result<OpenOutcome>>;
  /** Opens the project that holds a file in a window of its own, showing the file. */
  openInProject(path: string): Promise<Result<null>>;
  showWelcome(): Promise<Result<null>>;
  listWorkspaces(): Promise<Result<WorkspaceSummary[]>>;
  focusWorkspace(id: WorkspaceId): Promise<Result<null>>;
  recentList(): Promise<Result<RecentFolder[]>>;
  recentRemove(path: string): Promise<Result<null>>;
  settingsGet(): Promise<Result<AppSettings>>;
  settingsSet(change: SettingChange): Promise<Result<AppSettings>>;
  projectSessionGet(key: string): Promise<Result<JsonValue | null>>;
  projectSessionSet(key: string, value: JsonValue | null): Promise<Result<null>>;
  /** This window's Explorer list: `null` filter = the project's default. Changes arrive as `tree-changed`. */
  treeOpen(filter: TreeFilter | null): Promise<Result<TreeSnapshot>>;
  treeSetFilter(filter: TreeFilter): Promise<Result<TreeSnapshot>>;
  /** Lists a folder left unopened (large | ignored). */
  treeOpenFolder(path: string): Promise<Result<null>>;
  /** The open document stays in the list even when the filter hides it. */
  treeKeep(path: string | null): Promise<Result<null>>;
  treeClose(): Promise<Result<null>>;
  planMove(moves: MoveSpec[], updateLinks: boolean): Promise<Result<MovePlan>>;
  applyMove(planId: number, updateLinks: boolean): Promise<Result<MoveOutcome>>;
  deleteFiles(paths: string[]): Promise<Result<DeleteOutcome>>;
  /** A new empty file | folder in `dir`; resolves to its path. */
  newEntry(dir: string, name: string, kind: NewKind): Promise<Result<string>>;
  /** Takes the newest file operation back; `null` = nothing to undo. */
  undoFileOp(): Promise<Result<UndoOutcome | null>>;
  absPath(path: string): Promise<Result<string>>;
  openExternal(path: string, mode: OpenMode): Promise<Result<null>>;
  /** This window shows this document (`null`: none). */
  showDocument(path: string | null): Promise<Result<null>>;
  /** Reads + renders a document for this window. `known`: ids of the blocks it holds (their HTML is not sent again). */
  openDocument(path: string, known: string[]): Promise<Result<DocumentView>>;
  /** This window shows no document any more. */
  closeDocument(): Promise<Result<null>>;
  /** The text of a document as read (copy document). */
  documentText(path: string): Promise<Result<string>>;
  /** Where each link of a document leads. */
  resolveLinks(doc: string, hrefs: string[]): Promise<Result<LinkTarget[]>>;
  /** The project files each written code reference can mean. */
  resolveCodeRefs(doc: string, written: string[]): Promise<Result<string[][]>>;
  /** Opens a project file in the person's editor at a line. */
  openInEditor(path: string, line: number | null, col: number | null): Promise<Result<null>>;
  /** Opens a web link in the system browser (http | https | mailto only). */
  openWeb(url: string): Promise<Result<null>>;
  /** What an editor starts from: the text (LF), its hash, the block map (Markdown). */
  editSource(path: string): Promise<Result<EditSource>>;
  /** Block map of an unsaved Markdown text. */
  editSegments(text: string): Promise<Result<Segment[]>>;
  /** Line edits against the version `seenHash`; another version on disk = `conflict`, nothing written. */
  editSave(path: string, seenHash: string, edits: LineEdit[]): Promise<Result<SaveOutcome>>;
  /** The person's text as a new file next to `path` (`exact`: at `path`, when free). Resolves to its path. */
  editSaveAs(path: string, text: string, exact: boolean): Promise<Result<string>>;
  /** Picture bytes saved next to `doc`; resolves to the picture's path. */
  editWriteAsset(doc: string, ext: string, bytes: Uint8Array): Promise<Result<string>>;
  /** This window holds unsaved edits of `path` (`null`: none). */
  editSetUnsaved(path: string | null): Promise<Result<null>>;
  editHeadings(path: string): Promise<Result<OutlineItem[]>>;
  editLinkTargets(): Promise<Result<string[]>>;
  /** The sheet of an unsaved text of a config file (or the text, when it cannot be shown as a sheet). */
  configSheet(path: string, text: string): Promise<Result<Body>>;
  /** One value edit on the editor's text: the new text (only that value changed) and its sheet. */
  configEdit(path: string, text: string, edit: ConfigEdit): Promise<Result<ConfigEdited>>;
  /** Where the secret values of the text are (UTF-16 units). */
  configSecrets(path: string, text: string): Promise<Result<SecretRange[]>>;
  /** How many project files each pattern of an ignore file matches (`text`: the editor's version, else the disk's). */
  ignoreMatches(path: string, text: string | null): Promise<Result<IgnoreMatches>>;
  /** Measures of the enabled analyses, of a document (`path`) or a text. */
  analyze(path: string | null, text: string | null, kind: TextKind): Promise<Result<Measure[]>>;
  /** Where the project's search index is (the first build, then ready). */
  searchStatus(): Promise<Result<IndexStatus>>;
  /** Ranked sections for the words; `partial` while the first build runs. */
  searchSections(
    query: string,
    limit: number,
    options: SearchOptions,
  ): Promise<Result<SectionResults>>;
  /** Files (and folders) whose path matches the words, best first. */
  searchFiles(
    query: string,
    limit: number,
    folderLimit: number,
    options: SearchOptions,
  ): Promise<Result<PathResults>>;
  /** Quick open's empty box: the opened files that still exist, and the newest files. */
  searchRecent(opened: string[], limit: number, options: SearchOptions): Promise<Result<Recent>>;
  /** Calls a method a running plugin of this window's project serves. */
  pluginCall<M extends PluginMethod>(
    method: M,
    args: PluginMethods[M]["args"],
  ): Promise<Result<PluginMethods[M]["result"]>>;
  /** Listens to an event of the project plugins (every project's: the listener keeps its own folder's). */
  onPlugin<E extends PluginEventName>(
    event: E,
    handler: (payload: PluginEnvelope<E>) => void,
  ): Promise<Unlisten>;
  /** Closes this window for real (after the unsaved prompt). */
  windowClose(): Promise<Result<null>>;
  /** URL the webview loads a picture | sound | video of a workspace from. `version`: changes when the file does. */
  assetUrl(workspace: WorkspaceId, path: string, version: string | null): string;
  /** Listens to a backend event. Resolves to teardown. */
  on<E extends EventName>(event: E, handler: (payload: Events[E]) => void): Promise<Unlisten>;
  /** Drag + drop of files | folders onto this window. Resolves to teardown. */
  onDrop(handler: (event: DropEvent) => void): Promise<Unlisten>;
  /** System folder | file picker; `null` = cancelled. */
  pickFolder(title: string): Promise<string | null>;
  /** `anyFile`: no filter on the kind of file (the Notes open any text file). */
  pickFile(title: string, filterName: string, anyFile?: boolean): Promise<string | null>;
  setTitle(title: string): Promise<void>;
  /** Shows the window (created hidden: first paint already themed). */
  showWindow(): Promise<Result<void>>;
}
