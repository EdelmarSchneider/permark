// Backend interface. Implementations: `tauri.ts` (real IPC), `mock.ts` (dev:mock + tests). Only `api/` calls Tauri.

import type { AppInfo } from "./generated/AppInfo";
import type { AppSettings } from "./generated/AppSettings";
import type { BootInfo } from "./generated/BootInfo";
import type { CommandName } from "./generated/CommandName";
import type { ContextChanged } from "./generated/ContextChanged";
import type { DeleteOutcome } from "./generated/DeleteOutcome";
import type { IpcError } from "./generated/IpcError";
import type { MoveOutcome } from "./generated/MoveOutcome";
import type { MovePlan } from "./generated/MovePlan";
import type { MoveSpec } from "./generated/MoveSpec";
import type { NewKind } from "./generated/NewKind";
import type { OpenFailed } from "./generated/OpenFailed";
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

export type {
  AppInfo,
  CommandName,
  AppSettings,
  BootInfo,
  DeleteOutcome,
  IpcError,
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
}

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
  /** Listens to a backend event. Resolves to teardown. */
  on<E extends EventName>(event: E, handler: (payload: Events[E]) => void): Promise<Unlisten>;
  /** Drag + drop of files | folders onto this window. Resolves to teardown. */
  onDrop(handler: (event: DropEvent) => void): Promise<Unlisten>;
  /** System folder | file picker; `null` = cancelled. */
  pickFolder(title: string): Promise<string | null>;
  pickFile(title: string, filterName: string): Promise<string | null>;
  setTitle(title: string): Promise<void>;
  /** Shows the window (created hidden: first paint already themed). */
  showWindow(): Promise<Result<void>>;
}
