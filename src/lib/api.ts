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
// Mock implementation: plausible fake data, no backend needed
// ---------------------------------------------------------------------------------------------

const MOCK_ROOT = "C:/Users/demo/notes";
const MOCK_NOW = Date.now();
const MINUTE = 60_000;

const mockVault: VaultInfo = {
  root: MOCK_ROOT,
  name: "notes",
  mode: "ide",
  initialFile: "README.md",
  worktrees: [{ name: "feature-search", path: "C:/Users/demo/notes-wt/feature-search" }],
};

const mockTree: TreeNode = {
  name: "notes",
  path: "",
  kind: "dir",
  children: [
    {
      name: "guide",
      path: "guide",
      kind: "dir",
      children: [
        { name: "getting-started.md", path: "guide/getting-started.md", kind: "file" },
        { name: "architecture.md", path: "guide/architecture.md", kind: "file" },
      ],
    },
    {
      name: "ideas",
      path: "ideas",
      kind: "dir",
      children: [{ name: "new-feature.md", path: "ideas/new-feature.md", kind: "file" }],
    },
    { name: "config.json", path: "config.json", kind: "file" },
    { name: "README.md", path: "README.md", kind: "file" },
  ],
};

const mockWorktreeRoot: TreeNode = {
  name: "feature-search",
  path: "@wt/feature-search",
  kind: "worktree",
  children: [
    {
      name: "README.md",
      path: "@wt/feature-search/README.md",
      kind: "file",
    },
  ],
};

/** Small stable string hash, so equal blocks get equal hashes (like the real per-block hashes). */
function hashBlock(html: string): string {
  let h = 5381;
  for (let i = 0; i < html.length; i++) {
    h = ((h << 5) + h + html.charCodeAt(i)) >>> 0;
  }
  return h.toString(16).padStart(8, "0");
}

function mockDocument(path: string, title: string, blocks: string[]): DocumentView {
  return {
    path,
    kind: "markdown",
    html: blocks.join("\n"),
    blocks: blocks.map(hashBlock),
    title,
    frontmatter: null,
    modifiedMs: MOCK_NOW - 12 * MINUTE,
    encodingNote: null,
  };
}

const mockDocuments: Record<string, DocumentView> = {
  "README.md": mockDocument("README.md", "Notes", [
    '<h1 id="notes">Notes</h1>',
    '<p>Start with the <a href="guide/getting-started.md">getting started guide</a>, then read the <a href="guide/architecture.md#overview">architecture overview</a>.</p>',
    '<ul><li><input type="checkbox" checked disabled /> Write the guide</li><li><input type="checkbox" disabled /> Review the ideas</li></ul>',
  ]),
  "guide/getting-started.md": mockDocument("guide/getting-started.md", "Getting started", [
    '<h1 id="getting-started">Getting started</h1>',
    "<p>Open a folder and the tree appears on the left.</p>",
    "<table><thead><tr><th>Shortcut</th><th>Action</th></tr></thead><tbody><tr><td><kbd>Ctrl</kbd>+<kbd>P</kbd></td><td>Quick open</td></tr><tr><td><kbd>Ctrl</kbd>+<kbd>F</kbd></td><td>Find in page</td></tr></tbody></table>",
    '<pre><code class="language-rust">fn main() {\n    println!("hello");\n}\n</code></pre>',
  ]),
  "guide/architecture.md": mockDocument("guide/architecture.md", "Architecture", [
    '<h1 id="architecture">Architecture</h1>',
    '<h2 id="overview">Overview</h2>',
    "<p>The backend watches the folder and the UI follows the changes.</p>",
    '<pre><code class="language-mermaid">graph LR\n  Disk --&gt; Watcher --&gt; UI\n</code></pre>',
  ]),
  "ideas/new-feature.md": mockDocument("ideas/new-feature.md", "New feature", [
    '<h1 id="new-feature">New feature</h1>',
    "<p>A draft that is still untracked.</p>",
  ]),
};

const mockSession: SessionState = {
  lastDoc: "README.md",
  scrollTop: 0,
  zoom: 1,
  wide: false,
  refTemplate: "@{path}",
  leftPanel: { open: true, width: 260 },
  rightPanel: { open: true, width: 280 },
  theme: "system",
  follow: false,
};

function mockSnapshot(kind: "lastVisit" | "head", generation: number): GitSnapshot {
  return {
    generation,
    isRepo: true,
    gitError: null,
    head: {
      branch: "main",
      oid: "3f2a9c1d8e7b6a5f4c3d2e1f0a9b8c7d6e5f4a3b",
      detached: false,
      unborn: false,
    },
    repoState: "clean",
    baseline: { kind, oid: kind === "head" ? "3f2a9c1d8e7b6a5f4c3d2e1f0a9b8c7d6e5f4a3b" : null },
    files: {
      "guide/getting-started.md": { state: "modified", staged: false },
      "ideas/new-feature.md": { state: "untracked", staged: false },
    },
    truncated: false,
  };
}

function escapeHtml(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

type AnyHandler = (payload: never) => void;
const mockListeners = new Map<EventName, Set<AnyHandler>>();

/** Lets UI code and tests simulate a backend event while running against `mockApi`. */
export function emitMock<E extends EventName>(event: E, payload: EventMap[E]): void {
  for (const handler of mockListeners.get(event) ?? []) {
    (handler as (p: EventMap[E]) => void)(payload);
  }
}

let mockBaseline: "lastVisit" | "head" = "lastVisit";
let mockGeneration = 1;
let mockSavedSession: SessionState | null = mockSession;

export const mockApi: Api = {
  openVault: async () => mockVault,
  initialPath: async () => null,
  recentVaults: async () => [
    { root: MOCK_ROOT, name: "notes", lastOpenedMs: MOCK_NOW - 5 * MINUTE },
    {
      root: "C:/Users/demo/projects/api-docs",
      name: "api-docs",
      lastOpenedMs: MOCK_NOW - 3 * 24 * 60 * MINUTE,
    },
  ],
  listTree: async () => ({
    ...mockTree,
    children: [...(mockTree.children ?? []), mockWorktreeRoot],
  }),
  readDocument: async (path) =>
    mockDocuments[path] ??
    mockDocument(path, path, [
      `<h1 id="mock">${escapeHtml(path)}</h1>`,
      "<p>Mock content for a document without fixture data.</p>",
    ]),
  resolveLink: async (from, href) => {
    if (href.startsWith("#")) return { kind: "anchor", anchor: href.slice(1) };
    if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return { kind: "external", url: href };
    const [target, anchor] = href.split("#");
    const dir = from.includes("/") ? from.slice(0, from.lastIndexOf("/") + 1) : "";
    const path = target.startsWith("/") ? target.slice(1) : dir + target;
    return path in mockDocuments
      ? { kind: "internal", path, anchor: anchor ?? null }
      : { kind: "missing", path };
  },
  gitSnapshot: async () => mockSnapshot(mockBaseline, mockGeneration),
  setGitBaseline: async (kind) => {
    mockBaseline = kind;
    mockGeneration += 1;
    return mockSnapshot(mockBaseline, mockGeneration);
  },
  sinceLastVisit: async () => ({
    sinceMs: MOCK_NOW - 3 * 60 * MINUTE,
    files: [
      { path: "guide/getting-started.md", state: "modified", modifiedMs: MOCK_NOW - 12 * MINUTE },
      { path: "ideas/new-feature.md", state: "untracked", modifiedMs: MOCK_NOW - 45 * MINUTE },
    ],
  }),
  saveSession: async (s) => {
    mockSavedSession = s;
  },
  loadSession: async () => mockSavedSession,
  absolutePath: async (path) => `${MOCK_ROOT}/${path}`,
  search: async (query, limit = 20) => {
    const q = query.trim();
    if (q === "") return [];
    const hits: SearchHit[] = [
      {
        path: "guide/architecture.md",
        headingPath: ["Architecture", "Overview"],
        snippet: `The backend watches the folder, <mark>${escapeHtml(q)}</mark> and the UI follows the changes.`,
        startLine: 3,
        score: 4.2,
      },
      {
        path: "guide/getting-started.md",
        headingPath: ["Getting started"],
        snippet: `Open a folder and the tree appears, <mark>${escapeHtml(q)}</mark> on the left.`,
        startLine: 1,
        score: 2.7,
      },
    ];
    return hits.slice(0, limit);
  },
  initProjectFolder: async () => {},
  stateLocation: async () => "appData",
  on: async (event, handler) => {
    const handlers = mockListeners.get(event) ?? new Set<AnyHandler>();
    handlers.add(handler as AnyHandler);
    mockListeners.set(event, handlers);
    return () => {
      handlers.delete(handler as AnyHandler);
    };
  },
};

/** The API the app uses: the mock when built with `VITE_MOCK=1`, the Tauri backend otherwise. */
export const api: Api = import.meta.env.VITE_MOCK === "1" ? mockApi : tauriApi;
export default api;
