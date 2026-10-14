// The ONLY module that talks to the backend. Everything else in the UI imports `api` from here.
// Command names are the snake_case form of the camelCase methods.
// `VITE_MOCK=1` selects `mockApi`, which lets the UI be built and run in a browser without Rust.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ChangedSince,
  DocumentView,
  FsBatch,
  GitSnapshot,
  LinkTarget,
  RecentVault,
  SearchHit,
  SessionState,
  TreeNode,
  VaultInfo,
} from "./types";

/** Payloads of the events emitted by the backend. */
export type EventMap = {
  "fs-change": FsBatch;
  "tree-changed": null;
  "git-status-changed": GitSnapshot;
  "index-progress": { indexed: number; total: number };
};
export type EventName = keyof EventMap;
export type Unlisten = () => void;

export interface Api {
  openVault(path: string): Promise<VaultInfo>;
  /** The CLI argument, if any. */
  initialPath(): Promise<string | null>;
  recentVaults(): Promise<RecentVault[]>;
  /** Main root plus worktrees as top-level roots. */
  listTree(): Promise<TreeNode>;
  readDocument(path: string): Promise<DocumentView>;
  resolveLink(from: string, href: string): Promise<LinkTarget>;
  gitSnapshot(): Promise<GitSnapshot>;
  setGitBaseline(kind: "lastVisit" | "head"): Promise<GitSnapshot>;
  sinceLastVisit(): Promise<ChangedSince>;
  /** Last doc, scroll, zoom, theme, follow. */
  saveSession(s: SessionState): Promise<void>;
  loadSession(): Promise<SessionState | null>;
  /** For `convertFileSrc` and the external editor. */
  absolutePath(path: string): Promise<string>;
  search(query: string, limit?: number): Promise<SearchHit[]>;
  /** Creates `.trailmark/` in the project. */
  initProjectFolder(): Promise<void>;
  stateLocation(): Promise<"appData" | "project">;
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
  listTree: () => invoke<TreeNode>("list_tree"),
  readDocument: (path) => invoke<DocumentView>("read_document", { path }),
  resolveLink: (from, href) => invoke<LinkTarget>("resolve_link", { from, href }),
  gitSnapshot: () => invoke<GitSnapshot>("git_snapshot"),
  setGitBaseline: (kind) => invoke<GitSnapshot>("set_git_baseline", { kind }),
  sinceLastVisit: () => invoke<ChangedSince>("since_last_visit"),
  saveSession: (s) => invoke<void>("save_session", { s }),
  loadSession: () => invoke<SessionState | null>("load_session"),
  absolutePath: (path) => invoke<string>("absolute_path", { path }),
  search: (query, limit) => invoke<SearchHit[]>("search", { query, limit }),
  initProjectFolder: () => invoke<void>("init_project_folder"),
  stateLocation: () => invoke<"appData" | "project">("state_location"),
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
    listTree: command("listTree"),
    readDocument: command("readDocument"),
    resolveLink: command("resolveLink"),
    gitSnapshot: command("gitSnapshot"),
    setGitBaseline: command("setGitBaseline"),
    sinceLastVisit: command("sinceLastVisit"),
    saveSession: command("saveSession"),
    loadSession: command("loadSession"),
    absolutePath: command("absolutePath"),
    search: command("search"),
    initProjectFolder: command("initProjectFolder"),
    stateLocation: command("stateLocation"),
    on: async (event, handler) => addMockListener(event, handler),
  };
}

export const mockApi: Api = /* @__PURE__ */ createMockApi();

/** The API the app uses: the mock when built with `VITE_MOCK=1`, the Tauri backend otherwise. */
export const api: Api = import.meta.env.VITE_MOCK === "1" ? mockApi : tauriApi;
export default api;
