// Backend interface. Implementations: `tauri.ts` (real IPC), `mock.ts` (dev:mock + tests). Only `api/` calls Tauri.

import type { AppInfo } from "./generated/AppInfo";
import type { AppSettings } from "./generated/AppSettings";
import type { BootInfo } from "./generated/BootInfo";
import type { CommandName } from "./generated/CommandName";
import type { DefaultAppView } from "./generated/DefaultAppView";
import type { FileGroup } from "./generated/FileGroup";
import type { ShellEntry } from "./generated/ShellEntry";
import type { ShellState } from "./generated/ShellState";
import type { UpdateState } from "./generated/UpdateState";
import type { CountResult } from "./generated/CountResult";
import type { TokenCountState } from "./generated/TokenCountState";
import type { FolderBadge } from "./generated/FolderBadge";
import type { FolderBadges } from "./generated/FolderBadges";
import type { FolderJob } from "./generated/FolderJob";
import type { FolderSummary } from "./generated/FolderSummary";
import type { Analysis } from "./generated/Analysis";
import type { ConvertAnswer } from "./generated/ConvertAnswer";
import type { TargetView } from "./generated/TargetView";
import type { Problem } from "./generated/Problem";
import type { Table } from "./generated/Table";
import type { TableRow } from "./generated/TableRow";
import type { ContextChanged } from "./generated/ContextChanged";
import type { CloseRequested } from "./generated/CloseRequested";
import type { OpenIn } from "./generated/OpenIn";
import type { TreeShow } from "./generated/TreeShow";
import type { PluginKind } from "./generated/PluginKind";
import type { PluginInfo } from "./generated/PluginInfo";
import type { ProjectSettingChange } from "./generated/ProjectSettingChange";
import type { ProjectSettings } from "./generated/ProjectSettings";
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
import type { TextChunk } from "./generated/TextChunk";
import type { IndexProgress } from "./generated/IndexProgress";
import type { AgentChoice } from "./generated/AgentChoice";
import type { CardChange } from "./generated/CardChange";
import type { CardDto } from "./generated/CardDto";
import type { CardField } from "./generated/CardField";
import type { CardPlace } from "./generated/CardPlace";
import type { CardTarget } from "./generated/CardTarget";
import type { CardWarning } from "./generated/CardWarning";
import type { HiddenRule } from "./generated/HiddenRule";
import type { IncludeOutcome } from "./generated/IncludeOutcome";
import type { InnerLine } from "./generated/InnerLine";
import type { LocateRefusal } from "./generated/LocateRefusal";
import type { MemberState } from "./generated/MemberState";
import type { MissingReason } from "./generated/MissingReason";
import type { MoveAsk } from "./generated/MoveAsk";
import type { OfferSummary } from "./generated/OfferSummary";
import type { ProjectPick } from "./generated/ProjectPick";
import type { ProjectEvent } from "./generated/ProjectEvent";
import type { ProjectMarker } from "./generated/ProjectMarker";
import type { ProjectRow } from "./generated/ProjectRow";
import type { ProjectUid } from "./generated/ProjectUid";
import type { ProjectsChanged } from "./generated/ProjectsChanged";
import type { ProjectsUndone } from "./generated/ProjectsUndone";
import type { ProjectsView } from "./generated/ProjectsView";
import type { RecordSkip } from "./generated/RecordSkip";
import type { RecordStatus } from "./generated/RecordStatus";
import type { RootAnswer } from "./generated/RootAnswer";
import type { TagStatus } from "./generated/TagStatus";
import type { VocabularyEntry } from "./generated/VocabularyEntry";
import type { VocabularyKind } from "./generated/VocabularyKind";
import type { VocabularyStatus } from "./generated/VocabularyStatus";
import type { VocabularyView } from "./generated/VocabularyView";
import type { SkippedEntry } from "./generated/SkippedEntry";
import type { IndexStatus } from "./generated/IndexStatus";
import type { IpcError } from "./generated/IpcError";
import type { LinkTarget } from "./generated/LinkTarget";
import type { ConflictChoice } from "./generated/ConflictChoice";
import type { ImportFailure } from "./generated/ImportFailure";
import type { ImportItem } from "./generated/ImportItem";
import type { ImportKind } from "./generated/ImportKind";
import type { ImportMode } from "./generated/ImportMode";
import type { ImportOutcome } from "./generated/ImportOutcome";
import type { DropOpened } from "./generated/DropOpened";
import type { DropScope } from "./generated/DropScope";
import type { ImportPhase } from "./generated/ImportPhase";
import type { ImportPlan } from "./generated/ImportPlan";
import type { ImportProgress } from "./generated/ImportProgress";
import type { ImportProgressEvent } from "./generated/ImportProgressEvent";
import type { ImportRefusal } from "./generated/ImportRefusal";
import type { ImportRefused } from "./generated/ImportRefused";
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
import type { WindowsChanged } from "./generated/WindowsChanged";
import type { WindowSummary } from "./generated/WindowSummary";
import type { TabSummary } from "./generated/TabSummary";
import type { TabId } from "./generated/TabId";
import type { ClosingTab } from "./generated/ClosingTab";
import type { CloseAnswer } from "./generated/CloseAnswer";
import type { BarTheme } from "./generated/BarTheme";
import type { ViewRequest } from "./generated/ViewRequest";
import type { ViewMoved } from "./generated/ViewMoved";
import type { Basis } from "./generated/Basis";
import type { BaselineKind } from "./generated/BaselineKind";
import type { FeedItem } from "./generated/FeedItem";
import type { GitFile } from "./generated/GitFile";
import type { GitSnapshot } from "./generated/GitSnapshot";
import type { CommitLine } from "./generated/CommitLine";
import type { InterestLevel } from "./generated/InterestLevel";
import type { ProjectSnapshot } from "./generated/ProjectSnapshot";
import type { RepoError } from "./generated/RepoError";
import type { RepoSummary } from "./generated/RepoSummary";
import type { ReposChanged } from "./generated/ReposChanged";
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
import type { ActivityEntry } from "./generated/ActivityEntry";
import type { ActivityFact } from "./generated/ActivityFact";
import type { HandOffDone } from "./generated/HandOffDone";
import type { HandOffPreview } from "./generated/HandOffPreview";
import type { HandOffSessionView } from "./generated/HandOffSessionView";
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
import type { NotesIntent } from "./generated/NotesIntent";
import type { NotesSummary } from "./generated/NotesSummary";
import type { Version } from "./generated/Version";

export type {
  DropOpened,
  DropScope,
  AppInfo,
  CloseRequested,
  OpenIn,
  TreeShow,
  PluginInfo,
  PluginKind,
  ProjectSettingChange,
  ProjectSettings,
  ViewRequest,
  ClosingTab,
  CloseAnswer,
  BarTheme,
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
  TextChunk,
  ConflictChoice,
  ImportFailure,
  ImportItem,
  ImportKind,
  ImportMode,
  ImportOutcome,
  ImportPhase,
  ImportPlan,
  ImportProgress,
  ImportProgressEvent,
  ImportRefusal,
  ImportRefused,
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
  WindowSummary,
  TabSummary,
  TabId,
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
/** Projects inside the folder (`projects.md`). */
export type {
  AgentChoice,
  CardChange,
  CardDto,
  CardField,
  CardPlace,
  CardTarget,
  CardWarning,
  HiddenRule,
  IncludeOutcome,
  InnerLine,
  LocateRefusal,
  MemberState,
  MissingReason,
  MoveAsk,
  OfferSummary,
  ProjectEvent,
  ProjectPick,
  ProjectMarker,
  ProjectRow,
  ProjectUid,
  ProjectsChanged,
  ProjectsUndone,
  ProjectsView,
  RecordSkip,
  RecordStatus,
  RootAnswer,
  TagStatus,
  VocabularyEntry,
  VocabularyKind,
  VocabularyStatus,
  VocabularyView,
  SkippedEntry,
};

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
  list_windows: { args: Record<string, never>; result: WindowSummary[] };
  focus_workspace: { args: { id: WorkspaceId }; result: null };
  tab_activate: { args: { tab: TabId }; result: null };
  tab_close: { args: { tab: TabId }; result: null };
  tab_close_confirmed: { args: Record<string, never>; result: null };
  tab_move_new_window: { args: { tab: TabId }; result: string };
  tab_move: { args: { tab: TabId; window: string }; result: string };
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
  projects_view: { args: Record<string, never>; result: ProjectsView };
  projects_include: { args: { pick: ProjectPick }; result: IncludeOutcome };
  projects_decline: { args: { pick: ProjectPick }; result: null };
  projects_undo: { args: { token: number }; result: ProjectsUndone };
  projects_leave_out: { args: { uids: ProjectUid[] }; result: null };
  projects_set_hidden: { args: { uid: ProjectUid; rule: HiddenRule }; result: null };
  projects_set_agent: { args: { uid: ProjectUid; choice: AgentChoice }; result: null };
  projects_locate: { args: { uid: ProjectUid; path: string }; result: LocateRefusal | null };
  projects_remove: { args: { uids: ProjectUid[] }; result: null };
  projects_answer_move: { args: { uid: ProjectUid; path: string; same: boolean }; result: null };
  projects_card_set: { args: { target: CardTarget; change: CardChange }; result: CardDto };
  vocabulary_promote: { args: { kind: VocabularyKind; name: string }; result: null };
  fileops_plan_move: { args: { moves: MoveSpec[]; updateLinks: boolean }; result: MovePlan };
  fileops_apply_move: { args: { planId: number; updateLinks: boolean }; result: MoveOutcome };
  fileops_delete: { args: { paths: string[] }; result: DeleteOutcome };
  fileops_new: { args: { dir: string; name: string; kind: NewKind }; result: string };
  fileops_undo: { args: Record<string, never>; result: UndoOutcome | null };
  fileops_undo_entry: { args: { id: number }; result: UndoOutcome | null };
  fileops_import_plan: { args: { target: string; sources: string[] }; result: ImportPlan };
  fileops_import_apply: {
    args: { planId: number; mode: ImportMode; conflicts: ConflictChoice };
    result: ImportOutcome;
  };
  fileops_import_cancel: { args: { planId: number }; result: boolean };
  file_abs_path: { args: { path: string }; result: string };
  file_open_external: { args: { path: string; mode: OpenMode }; result: null };
  show_document: { args: { path: string | null }; result: null };
  document_open: { args: { path: string; known: string[] }; result: DocumentView };
  document_close: { args: Record<string, never>; result: null };
  document_text: { args: { path: string }; result: string };
  document_more: {
    args: { path: string; offset: number; size: number; mtimeMs: number };
    result: TextChunk;
  };
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
  /** A picture dropped from the system onto an editor: the file's bytes (raw response). */
  picture_read: { args: { path: string }; result: ArrayBuffer };
  /** A picture on the web the person asked for: its bytes (raw response). */
  remote_picture: { args: { url: string }; result: ArrayBuffer };
  remote_pictures_get: { args: Record<string, never>; result: boolean };
  remote_pictures_set: { args: { always: boolean }; result: null };
  edit_set_unsaved: { args: { path: string | null }; result: null };
  edit_headings: { args: { path: string }; result: OutlineItem[] };
  edit_link_targets: { args: Record<string, never>; result: string[] };
  config_sheet: { args: { path: string; text: string }; result: Body };
  config_edit: { args: { path: string; text: string; edit: ConfigEdit }; result: ConfigEdited };
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
  project_init: { args: { path: string }; result: null };
  project_config_get: { args: Record<string, never>; result: ProjectSettings };
  project_config_set: { args: { change: ProjectSettingChange }; result: ProjectSettings };
  plugins_list: { args: Record<string, never>; result: PluginInfo[] };
  plugin_set_project: { args: { plugin: string; enabled: boolean | null }; result: null };
  plugin_remove_data: { args: { plugin: string }; result: null };
  window_close: { args: Record<string, never>; result: null };
  window_close_answer: { args: { answer: CloseAnswer }; result: null };
  view_saved: { args: { saved: boolean }; result: null };
  view_ready: { args: Record<string, never>; result: null };
  view_set_title: { args: { title: string }; result: null };
  view_set_theme: { args: { theme: BarTheme | null }; result: null };
  open_dropped: { args: { paths: string[]; scope: DropScope }; result: DropOpened };
  view_focus: { args: Record<string, never>; result: null };
  default_app_get: { args: Record<string, never>; result: DefaultAppView };
  default_app_set: { args: { group: FileGroup; on: boolean }; result: DefaultAppView };
  default_app_asked: { args: Record<string, never>; result: null };
  default_app_handoff: { args: Record<string, never>; result: null };
}

/**
 * What running plugins serve (`plugin_call`): `<plugin>.<method>` -> args + result. Mirror of the plugins' `rpc`
 * registrations; DTOs from `generated/`. A plugin that is off answers `unavailable`.
 */
export interface PluginMethods {
  /** `project`: an included project's uid (its repository; `null` until a view declared interest); absent = root. */
  "git.snapshot": { args: { project?: string | null }; result: GitSnapshot | null };
  /** Window focus: absent = the root + every repository a view looks at; `project` = that one. */
  "git.refresh": { args: { project?: string | null }; result: null };
  /** Repositories of the included projects (cheap facts at once, counts | details as known). */
  "git.repos": { args: Record<string, never>; result: RepoSummary[] };
  /**
   * What this view (`view`: a tab id; absent = the window) wants to know at `level`: replaces its set for that level.
   * Send `[]` when the view goes | hides (nothing else drops it).
   */
  "git.interest": { args: { ids: string[]; level: InterestLevel; view?: string }; result: null };
  "git.setBaseline": { args: { kind: BaselineKind }; result: null };
  "live-feed.items": { args: Record<string, never>; result: FeedItem[] };
  "live-feed.clear": { args: Record<string, never>; result: null };
  "live-feed.activity": { args: Record<string, never>; result: ActivityEntry[] };
  "unread.list": { args: Record<string, never>; result: UnreadList };
  "unread.sinceLastVisit": { args: Record<string, never>; result: SinceVisit };
  "unread.markRead": { args: { path: string; hash: string }; result: null };
  "unread.markUnread": { args: { path: string }; result: null };
  "unread.markFolderRead": { args: { folder: string }; result: null };
  "review.status": { args: { path: string; basis?: Basis }; result: ReviewStatus };
  "review.seen": { args: { path: string; hash: string }; result: null };
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
  /** What the Notes tab shows next (kept until it takes it; every view hears `notes:intent`). */
  "notes.want": { args: { intent: NotesIntent }; result: null };
  "notes.takeIntent": { args: Record<string, never>; result: NotesIntent | null };
  "agent.status": { args: Record<string, never>; result: AgentStatus };
  "agent.setEnabled": { args: { enabled: boolean }; result: AgentStatus };
  "agent.setGroup": { args: { group: string; on: boolean }; result: AgentStatus };
  "agent.setDefaults": { args: Record<string, never>; result: null };
  "agent.publish": { args: { context: AgentWindowContext }; result: null };
  "agent.setup": { args: Record<string, never>; result: string | null };
  "agent.handOffPreview": { args: { checked?: number[] }; result: HandOffPreview };
  "agent.handOffConfirm": { args: { upto: number; checked: number[] }; result: HandOffDone };
  "shell-integration.state": { args: Record<string, never>; result: ShellState };
  "shell-integration.set": { args: { entry: ShellEntry; on: boolean }; result: ShellState };
  "updates.state": { args: Record<string, never>; result: UpdateState };
  "updates.check": { args: Record<string, never>; result: UpdateState };
  "updates.download": { args: Record<string, never>; result: UpdateState };
  "updates.restart": { args: Record<string, never>; result: UpdateState };
  "updates.dismiss": { args: Record<string, never>; result: UpdateState };
  "token-count.state": { args: Record<string, never>; result: TokenCountState };
  "token-count.download": { args: { tokenizer: string }; result: TokenCountState };
  "token-count.cancel": { args: { tokenizer: string }; result: TokenCountState };
  "token-count.removeVocab": { args: { tokenizer: string }; result: TokenCountState };
  "token-count.setOutline": { args: { on: boolean }; result: TokenCountState };
  "token-count.setFolderTotals": { args: { on: boolean }; result: TokenCountState };
  "token-count.folderStart": {
    args: { folder: string; tokenizer: string; auto?: boolean };
    result: FolderJob;
  };
  "token-count.folderConfirm": { args: Record<string, never>; result: FolderJob };
  "token-count.folderCancel": { args: Record<string, never>; result: FolderJob };
  "token-count.folderDismiss": { args: Record<string, never>; result: FolderJob };
  "token-count.folderJob": { args: Record<string, never>; result: FolderJob };
  "token-count.folders": { args: { root: string }; result: FolderBadges };
  "token-count.folderSummary": {
    args: { root: string; folder: string };
    result: FolderSummary | null;
  };
  "token-count.count": {
    args: { tokenizers: string[]; text: string; parts?: [number, number][] };
    result: CountResult[];
  };
  "fmt-toon.toJson": { args: { text: string }; result: string };
  "convert.analyze": {
    args: { job: number; source: ConvertSource; from?: string };
    result: ConvertAnswer;
  };
  "convert.result": {
    args: { job: number; target: string; confirmed?: boolean };
    result: string;
  };
  "convert.save": {
    args: { job: number; target: string; confirmed?: boolean; beside?: string };
    result: string;
  };
  "convert.cancel": { args: { job: number }; result: null };
  "convert.close": { args: { job: number }; result: null };
}

/** What the converter reads: a file of the workspace, or a text from the page. */
export type ConvertSource = { kind: "file"; path: string } | { kind: "text"; text: string };
export type PluginMethod = keyof PluginMethods;

/** Events of project plugins: `{ root, data }` (the folder they are about; a window keeps its own). */
export interface PluginEvents {
  "git:snapshot": GitSnapshot | null;
  "git:repos": ReposChanged;
  "git:projectSnapshot": ProjectSnapshot;
  "live-feed:items": { upsert: FeedItem[] };
  "live-feed:activity": { entry: ActivityEntry };
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

/** Events of app plugins: the payload as is (no folder: one instance for the whole app). */
export interface AppPluginEvents {
  "updates:state": UpdateState;
  "token-count:state": TokenCountState;
  /** One window's folder count: what its job is doing. */
  "token-count:folder": { window: string; job: FolderJob };
  /** The counted folders of a project changed (a result, a stale mark). */
  "token-count:folders": FolderBadges;
  /** Something asked the Notes tab to show a note | a new note: the Notes tab takes it (`notes.takeIntent`). */
  "notes:intent": null;
}
export type AppPluginEventName = keyof AppPluginEvents;
export type { DefaultAppView, FileGroup, ShellEntry, ShellState, UpdateState };
export type { CountResult, TokenCountState };
export type { FolderBadge, FolderBadges, FolderJob, FolderSummary };
export type { FolderSkipped } from "./generated/FolderSkipped";
export type { FolderTopFile } from "./generated/FolderTopFile";
export type { FolderTypeRow } from "./generated/FolderTypeRow";
export type { FolderSkip } from "./generated/FolderSkip";
export type { Analysis, ConvertAnswer, TargetView, Problem, Table, TableRow };
export type { VocabEntry } from "./generated/VocabEntry";
export type { VocabFailure } from "./generated/VocabFailure";
export type { VocabState } from "./generated/VocabState";
export type { DefaultAppMode } from "./generated/DefaultAppMode";
export type { GroupView } from "./generated/GroupView";
export type { UpdateStage } from "./generated/UpdateStage";

export type {
  Basis,
  BaselineKind,
  FeedItem,
  GitFile,
  GitSnapshot,
  CommitLine,
  InterestLevel,
  ProjectSnapshot,
  RepoError,
  RepoSummary,
  ReposChanged,
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
export type { ActivityEntry, ActivityFact, HandOffDone, HandOffPreview, HandOffSessionView };
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
  NotesIntent,
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
  "windows-changed": WindowsChanged;
  "settings-changed": SettingsChanged;
  "open-failed": OpenFailed;
  "tree-changed": TreeChanged;
  "document-changed": DocumentChanged;
  "close-requested": CloseRequested;
  "tab-close-requested": ViewRequest;
  "save-requested": ViewRequest;
  "view-moved": ViewMoved;
  "plugins-changed": null;
  "index-progress": IndexProgress;
  /** The projects of a folder changed: the window showing `root` reads `projectsView` again. */
  "projects-changed": ProjectsChanged;
  "import-progress": ImportProgressEvent;
}
export type EventName = keyof Events;

export type ApiError =
  | { kind: "backend"; command: CommandName; error: IpcError }
  | { kind: "command"; command: CommandName; message: string }
  | { kind: "window"; message: string };

export type Result<T> = { ok: true; value: T } | { ok: false; error: ApiError };

export type Unlisten = () => void;

/** Files | folders dragged onto the window. `x`, `y`: where the pointer is, in CSS pixels of the window. */
export type DropEvent =
  | { type: "over"; count: number; paths: readonly string[]; x: number; y: number }
  | { type: "leave" }
  | { type: "drop"; paths: string[]; x: number; y: number };

export interface Api {
  appInfo(): Promise<Result<AppInfo>>;
  windowBoot(): Promise<Result<BootInfo>>;
  /** Opens a folder | file where it is already (one place), else by `placement`. */
  openPath(path: string, placement: OpenPlacement): Promise<Result<OpenOutcome>>;
  /** Opens the Notes (a tab; where they are already, else by `placement`). */
  openNotes(placement: OpenPlacement): Promise<Result<OpenOutcome>>;
  /** Opens the project that holds a file (a new tab of this window), showing the file. */
  openInProject(path: string): Promise<Result<null>>;
  /** Every window with its tabs (this view shows its own window's). */
  listWindows(): Promise<Result<WindowSummary[]>>;
  focusWorkspace(id: WorkspaceId): Promise<Result<null>>;
  tabActivate(tab: TabId): Promise<Result<null>>;
  /** Closes a tab (its view asks first when it holds unsaved edits: `tab-close-requested`). */
  tabClose(tab: TabId): Promise<Result<null>>;
  /** The unsaved prompt of this view's tab let it go: the tab closes. */
  tabCloseConfirmed(): Promise<Result<null>>;
  /** Moves a tab into a new window (its label). */
  tabMoveNewWindow(tab: TabId): Promise<Result<string>>;
  /** Moves a tab into another existing work window (its label). */
  tabMove(tab: TabId, window: string): Promise<Result<string>>;
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
  /** Projects inside this window's folder: Settings > Projects + the strip. Changes arrive as `projects-changed`. */
  projectsView(): Promise<Result<ProjectsView>>;
  /** Includes projects (`all` = every offered one); a folder without `.permark/` is initialized first. */
  projectsInclude(pick: ProjectPick): Promise<Result<IncludeOutcome>>;
  /** "No thanks": `all` on the first question declines for this folder; paths decline those (profile only). */
  projectsDecline(pick: ProjectPick): Promise<Result<null>>;
  /** Takes back the last include (its `undo` token). */
  projectsUndo(token: number): Promise<Result<ProjectsUndone>>;
  projectsLeaveOut(uids: ProjectUid[]): Promise<Result<null>>;
  projectsSetHidden(uid: ProjectUid, rule: HiddenRule): Promise<Result<null>>;
  projectsSetAgent(uid: ProjectUid, choice: AgentChoice): Promise<Result<null>>;
  /** Locate… of a missing project: `path` = the folder picked (absolute). `null` = done, else why not. */
  projectsLocate(uid: ProjectUid, path: string): Promise<Result<LocateRefusal | null>>;
  /** Missing projects taken off the list. */
  projectsRemove(uids: ProjectUid[]): Promise<Result<null>>;
  /** The move question: `same` = the project at `path` is the missing one. */
  projectsAnswerMove(uid: ProjectUid, path: string, same: boolean): Promise<Result<null>>;
  /** One field of a card (the folder's own | an included project's); an empty value removes the field. */
  projectsCardSet(target: CardTarget, change: CardChange): Promise<Result<CardDto>>;
  /**
   * Makes a tag | category official in this folder (`.permark/vocabulary.toml`; the local look is copied). Already
   * official: nothing to do. `unavailable` (subject `vocabulary`): no `.permark/` here, the list can't be read | is
   * newer. Changes arrive as `projects-changed` (+ `tags:changed`).
   */
  vocabularyPromote(kind: VocabularyKind, name: string): Promise<Result<null>>;
  planMove(moves: MoveSpec[], updateLinks: boolean): Promise<Result<MovePlan>>;
  applyMove(planId: number, updateLinks: boolean): Promise<Result<MoveOutcome>>;
  deleteFiles(paths: string[]): Promise<Result<DeleteOutcome>>;
  /** A new empty file | folder in `dir`; resolves to its path. */
  newEntry(dir: string, name: string, kind: NewKind): Promise<Result<string>>;
  /** Takes the newest file operation back; `null` = nothing to undo. */
  undoFileOp(): Promise<Result<UndoOutcome | null>>;
  /** Takes back exactly the operation `id` (an import notice's Undo); `null`: gone, or a newer operation touched it. */
  undoFileOpEntry(id: number): Promise<Result<UndoOutcome | null>>;
  /** What dropping files | folders from outside onto `target` (a project folder) would do. Touches nothing. */
  planImport(target: string, sources: string[]): Promise<Result<ImportPlan>>;
  /** Does what the plan listed; resolves when done (progress arrives as `import-progress`). */
  applyImport(
    planId: number,
    mode: ImportMode,
    conflicts: ConflictChoice,
  ): Promise<Result<ImportOutcome>>;
  /** Stops a running import; what was finished stays. */
  cancelImport(planId: number): Promise<Result<boolean>>;
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
  /** The next chunk of a large text, from byte `offset`; `size` and `mtimeMs` are the ones the view was opened with. */
  documentMore(
    path: string,
    offset: number,
    size: number,
    mtimeMs: number,
  ): Promise<Result<TextChunk>>;
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
  /** The bytes of a picture file dropped onto an editor (absolute path). */
  readPicture(path: string): Promise<Result<Uint8Array<ArrayBuffer>>>;
  /** A picture on the web, fetched now because the person asked (`https` only, picture types, <= 20 MB). */
  remotePicture(url: string): Promise<Result<Uint8Array<ArrayBuffer>>>;
  /** Web pictures load by themselves in this project (the person's choice, kept in the profile only). */
  remotePicturesGet(): Promise<Result<boolean>>;
  remotePicturesSet(always: boolean): Promise<Result<null>>;
  /** This window holds unsaved edits of `path` (`null`: none). */
  editSetUnsaved(path: string | null): Promise<Result<null>>;
  editHeadings(path: string): Promise<Result<OutlineItem[]>>;
  editLinkTargets(): Promise<Result<string[]>>;
  /** The sheet of an unsaved text of a config file (or the text, when it cannot be shown as a sheet). */
  configSheet(path: string, text: string): Promise<Result<Body>>;
  /** One value edit on the editor's text: the new text (only that value changed) and its sheet. */
  configEdit(path: string, text: string, edit: ConfigEdit): Promise<Result<ConfigEdited>>;
  /** Where the secret values of the text are (UTF-16 units). */
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
  /** Listens to an event of an app plugin (one instance for the app: no folder). */
  onAppPlugin<E extends AppPluginEventName>(
    event: E,
    handler: (payload: AppPluginEvents[E]) => void,
  ): Promise<Unlisten>;
  /** The default app: the rows of the settings and whether to ask now (first run). */
  defaultAppGet(): Promise<Result<DefaultAppView>>;
  /** Makes PerMark the default app of a group, or gives it back (macOS, an installed Linux package). */
  defaultAppSet(group: FileGroup, on: boolean): Promise<Result<DefaultAppView>>;
  /** The first-run question was answered or put off. */
  defaultAppAsked(): Promise<Result<null>>;
  /** Opens Windows' Default Apps page. */
  defaultAppHandoff(): Promise<Result<null>>;
  /** Creates `.permark/` in a folder; a project open on it reloads its state at once. */
  projectInit(path: string): Promise<Result<null>>;
  /** The shared settings (`.permark/config.toml`) of this window's project. */
  projectConfigGet(): Promise<Result<ProjectSettings>>;
  projectConfigSet(change: ProjectSettingChange): Promise<Result<ProjectSettings>>;
  /** Every plugin with its state for this window. */
  pluginsList(): Promise<Result<PluginInfo[]>>;
  /** The person's own choice for a plugin in this project (`null`: follow the app). */
  pluginSetProject(plugin: string, enabled: boolean | null): Promise<Result<null>>;
  /** Deletes what a plugin keeps; refused while it is on. */
  pluginRemoveData(plugin: string): Promise<Result<null>>;
  /** Asks this window to close (one prompt when tabs hold unsaved edits: `close-requested`). */
  windowClose(): Promise<Result<null>>;
  /** The answer to the window close prompt. */
  windowCloseAnswer(answer: CloseAnswer): Promise<Result<null>>;
  /** This view answered `save-requested`. */
  viewSaved(saved: boolean): Promise<Result<null>>;
  /** URL the webview loads a picture | sound | video of a workspace from. `version`: changes when the file does. */
  assetUrl(workspace: WorkspaceId, path: string, version: string | null): string;
  /** Listens to a backend event. Resolves to teardown. */
  on<E extends EventName>(event: E, handler: (payload: Events[E]) => void): Promise<Unlisten>;
  /** Drag + drop of files | folders onto this view (only the view on screen gets them). Resolves to teardown. */
  onDrop(handler: (event: DropEvent) => void): Promise<Unlisten>;
  /** Paths dropped on this view open as tabs of its window (`existingOnly`: only those open somewhere come forward;
   *  the others come back in `left`). One place; the last path's place ends active. */
  openDropped(paths: string[], scope: DropScope): Promise<Result<DropOpened>>;
  /** This view's window comes forward and the view takes the keyboard (before a drop question; may be refused). */
  focusView(): Promise<Result<null>>;
  /** System folder | file picker; `null` = cancelled. `from`: the folder it opens at (else the system's choice). */
  pickFolder(title: string, from?: string | null): Promise<string | null>;
  /** `anyFile`: no filter on the kind of file (the Notes open any text file). */
  pickFile(title: string, filterName: string, anyFile?: boolean): Promise<string | null>;
  /** Pictures chosen in the system's file dialog (PNG, JPG, GIF, WebP, SVG); absolute paths, none when cancelled. */
  pickPictures(title: string, filterName: string): Promise<string[]>;
  /** The window title this view wants (set while it is the tab on screen). */
  setTitle(title: string): Promise<void>;
  /** The native title bar follows the theme (`null`: the system's). */
  setWindowTheme(theme: "light" | "dark" | null): Promise<void>;
  /** This view painted its first frame (a window made hidden shows with it). */
  showWindow(): Promise<Result<void>>;
}
