// Mock backend (`npm run dev:mock`, tests): same interface, no Tauri. One window ("main"), in memory.

import { DEFAULT_FILTER } from "../core/explorer/filter";
import type { Context } from "./generated/Context";
import type { ErrorKind } from "./generated/ErrorKind";
import type { WindowView } from "./generated/WindowView";
import type { WorkspaceView } from "./generated/WorkspaceView";
import { mockAssetUrl, mockLinkTarget, mockNoteView, mockView } from "./mockDocs";
import { createMockConfig } from "./mockConfig";
import { createMockDistribution, type DistributionProbe } from "./mockDistribution";
import { createMockEditing, type MockEditProbe } from "./mockEdit";
import { createMockNotes, MOCK_NOTES_FOLDER } from "./mockNotes";
import { createMockOrganizing } from "./mockOrganizing";
import { createMockPlugins, type MockPluginProbe } from "./mockPlugins";
import { MockProject, MockRefusal, seedFiles } from "./mockFs";
import { mockFiles, mockRecent, mockSections } from "./mockSearch";
import { createMockShell, MOCK_MANIFESTS, type MockShell } from "./mockShell";
import type {
  Api,
  AppPluginEventName,
  AppPluginEvents,
  AppSettings,
  CommandName,
  EventName,
  Events,
  JsonValue,
  PluginEnvelope,
  PluginEventName,
  PluginEvents,
  RecentFolder,
  Renamed,
  Result,
  SettingChange,
  TreeFilter,
  TreeNode,
  TreeSnapshot,
  Unlisten,
  WorkspaceSummary,
} from "./types";

export const MOCK_VERSION = "0.0.0-mock";
export const MOCK_FOLDER = "C:/Users/demo/permark-docs";
export const MOCK_FILE = "C:/Users/demo/Downloads/proposal.md";

const ok = <T>(value: T): Promise<Result<T>> => Promise.resolve({ ok: true, value });

function defaultSettings(): AppSettings {
  return {
    language: "system",
    theme: "system",
    tone: "white",
    startIn: "welcome",
    linkUpdate: "ask",
    editMode: "edit",
    openIn: "ask",
    gitBackend: "automatic",
    notesFolder: "",
    referenceTemplate: "{path}#{section}",
    editor: "",
    keymap: {},
    plugins: {},
  };
}

/** The plugins the real build runs by default. */
function neutralContext(): Context {
  return {
    workspace: "welcome",
    git: { status: "noRepo" },
    permark: "notInitialized",
    agent: { status: "off" },
    liveUpdates: { status: "unavailable", reason: "noWorkspace" },
    distribution: "direct",
    platform: "windows",
    plugins: [],
  };
}

function nameOf(path: string): string {
  const parts = path.split(/[\\/]/).filter((p) => p !== "");
  return parts[parts.length - 1] ?? path;
}

export function applyChange(settings: AppSettings, change: SettingChange): AppSettings {
  switch (change.key) {
    case "language":
      return { ...settings, language: change.value };
    case "theme":
      return { ...settings, theme: change.value };
    case "tone":
      return { ...settings, tone: change.value };
    case "startIn":
      return { ...settings, startIn: change.value };
    case "linkUpdate":
      return { ...settings, linkUpdate: change.value };
    case "editMode":
      return { ...settings, editMode: change.value };
    case "openIn":
      return { ...settings, openIn: change.value };
    case "gitBackend":
      return { ...settings, gitBackend: change.value };
    case "notesFolder":
      return { ...settings, notesFolder: change.value };
    case "referenceTemplate":
      return { ...settings, referenceTemplate: change.value.trim() || "{path}#{section}" };
    case "editor":
      return { ...settings, editor: change.value };
    case "keymap": {
      const keymap = { ...settings.keymap };
      if (change.value.combo === null) delete keymap[change.value.command];
      else keymap[change.value.command] = change.value.combo;
      return { ...settings, keymap };
    }
    case "pluginEnabled": {
      const plugins = { ...settings.plugins };
      if (change.value.enabled === null) delete plugins[change.value.plugin];
      else plugins[change.value.plugin] = change.value.enabled;
      return { ...settings, plugins };
    }
    default:
      return change satisfies never;
  }
}

type TreeView = { filter: TreeFilter; list: TreeNode[]; seq: number; keep: string | null };

/** What the mock did outside the interface: tests read it. */
export type MockProbe = {
  /** Paths handed to `openExternal`, with the mode. */
  opened: { path: string; mode: string }[];
  /** Files handed to `openInEditor`. */
  editor: { path: string; line: number | null; col: number | null }[];
  /** URLs handed to `openWeb`. */
  web: string[];
  /** The document this window shows now, and the block ids it said it holds. */
  shown: { path: string | null; known: string[] };
  /** The project in memory. */
  project: MockProject;
  /** Editing: saves, unsaved flag, files as the editor sees them. */
  edit: MockEditProbe;
  /** The plugins' backend halves: calls they got, ways to make them report. */
  plugins: MockPluginProbe;
  /** The shell's backend half: the project's shared settings, other workspaces, what was asked. */
  shell: MockShell["probe"];
  /** Themes the window's title bar was asked to take (`null`: the system's). */
  windowThemes: ("light" | "dark" | null)[];
  /** The default app, the file manager entries, the updater. */
  distribution: DistributionProbe;
};

/** Fresh mock (tests get their own; the dev build uses `mockApi`). */
export function createMockApi(): Api & {
  emit<E extends EventName>(event: E, payload: Events[E]): void;
  probe: MockProbe;
} {
  let settings = defaultSettings();
  let view: WindowView = { state: "welcome" };
  let nextId = 1;
  let recent: RecentFolder[] = [
    {
      path: MOCK_FOLDER,
      name: nameOf(MOCK_FOLDER),
      openedMs: Date.now() - 3_600_000,
      missing: false,
    },
    { path: "D:/old/specs", name: "specs", openedMs: Date.now() - 86_400_000, missing: true },
  ];
  const session = new Map<string, JsonValue>();
  const listeners: { [E in EventName]: Set<(payload: Events[E]) => void> } = {
    "window-changed": new Set(),
    "context-changed": new Set(),
    "workspaces-changed": new Set(),
    "settings-changed": new Set(),
    "state-recovered": new Set(),
    "open-failed": new Set(),
    "tree-changed": new Set(),
    "document-changed": new Set(),
    "close-requested": new Set(),
    "workspace-close-requested": new Set(),
    "plugins-changed": new Set(),
    "index-progress": new Set(),
  };
  const pluginListeners: { [E in PluginEventName]: Set<(payload: PluginEnvelope<E>) => void> } = {
    "git:snapshot": new Set(),
    "live-feed:items": new Set(),
    "unread:changed": new Set(),
    "tags:changed": new Set(),
    "comments:changed": new Set(),
    "quiet:changed": new Set(),
    "broken-links:changed": new Set(),
    "agent:status": new Set(),
    "agent:open": new Set(),
    "agent:ask": new Set(),
  };
  /** The folder the window shows, as the events of its plugins name it. */
  function rootNow(): string {
    return view.state === "showing" && view.workspace.kind === "project" ? view.workspace.root : "";
  }
  function emitPlugin<E extends PluginEventName>(event: E, data: PluginEvents[E]): void {
    for (const handler of pluginListeners[event]) handler({ root: rootNow(), data });
    // the agent switch is part of the window's context, as in the app
    if (event === "agent:status") emit("context-changed", { window: "main", context: context() });
  }
  function inProject(): boolean {
    return view.state === "showing" && view.workspace.kind === "project";
  }

  /** The workspace this window shows, as the process lists it. */
  function ownSummary(): WorkspaceSummary | null {
    if (view.state !== "showing") return null;
    const workspace = view.workspace;
    switch (workspace.kind) {
      case "project":
        return {
          id: workspace.id,
          kind: "project",
          title: workspace.name,
          path: workspace.root,
          windows: 1,
        };
      case "singleFile":
        return {
          id: workspace.id,
          kind: "singleFile",
          title: workspace.name,
          path: workspace.path,
          windows: 1,
        };
      case "notes":
        return {
          id: workspace.id,
          kind: "notes",
          title: "Notes",
          path: workspace.folder ?? "",
          windows: 1,
        };
      default:
        return workspace satisfies never;
    }
  }

  /** The list of workspaces or the plugins moved: windows hear it, as from the backend. */
  function announceShell(): void {
    void shell.api.listWorkspaces().then((listed) => {
      if (listed.ok) emit("workspaces-changed", { workspaces: listed.value });
    });
    emit("plugins-changed", null);
    emit("context-changed", { window: "main", context: context() });
  }

  const shell = createMockShell({
    settings: () => settings,
    workspace: ownSummary,
    changed: announceShell,
    closeOwn: () => emit("workspace-close-requested", { window: "main" }),
  });
  const windowThemes: ("light" | "dark" | null)[] = [];

  const project = new MockProject();
  const organizing = createMockOrganizing(emitPlugin, project);
  const editing = createMockEditing(() => Object.keys(seedFiles()).sort());
  const notes = createMockNotes(editing.probe.files);
  const appListeners: { [E in AppPluginEventName]: Set<(payload: AppPluginEvents[E]) => void> } = {
    "updates:state": new Set(),
  };
  const distribution = createMockDistribution((event, payload) => {
    for (const handler of appListeners[event]) handler(payload);
  });
  const plugins = createMockPlugins(
    emitPlugin,
    () => mockView("docs/guide.md", [], true)?.hash ?? "0",
    organizing,
    notes,
    distribution.handlers,
  );
  const probe: MockProbe = {
    opened: [],
    editor: [],
    web: [],
    shown: { path: null, known: [] },
    project,
    edit: editing.probe,
    plugins: plugins.probe,
    shell: shell.probe,
    windowThemes,
    distribution: distribution.probe,
  };
  let tree: TreeView | null = null;

  function emit<E extends EventName>(event: E, payload: Events[E]): void {
    for (const handler of listeners[event]) handler(payload);
  }

  function fail<T>(command: CommandName, refusal: unknown): Result<T> {
    if (!(refusal instanceof MockRefusal)) throw refusal;
    const kind: ErrorKind = refusal.kind;
    const detail = kind === "invalidName" ? refusal.subject : null;
    return {
      ok: false,
      error: {
        kind: "backend",
        command,
        error: { kind, subject: kind === "invalidName" ? null : refusal.subject, detail },
      },
    };
  }

  /** Recomputes the window list and tells it the difference, like the backend does after a change. */
  function refreshTree(renamed: Renamed[] = []): void {
    if (!tree) return;
    const list = project.list(tree.filter);
    const { removed, added } = project.diff(tree.list, list);
    const counts = project.counts(tree.filter);
    tree = { ...tree, list, seq: tree.seq + 1 };
    emit("tree-changed", {
      window: "main",
      delta: {
        seq: tree.seq,
        removed,
        added,
        renamed,
        totalFiles: counts.total,
        shownFiles: counts.shown,
        bulk: false,
        filter: null,
      },
    });
  }

  function snapshot(filter: TreeFilter): TreeSnapshot {
    const list = project.list(filter);
    tree = { filter, list, seq: (tree?.seq ?? 0) + 1, keep: tree?.keep ?? null };
    const counts = project.counts(filter);
    return {
      roots: [{ name: "demo", kind: "main", path: "" }],
      nodes: list,
      filter,
      seq: tree.seq,
      invalidPatterns: [],
      totalFiles: counts.total,
      shownFiles: counts.shown,
    };
  }

  function context(): Context {
    const base = shell.context({ ...neutralContext(), plugins: shell.pluginsOn(inProject()) });
    if (view.state !== "showing") return base;
    const kinds = { project: "project", singleFile: "singleFile", notes: "notes" } as const;
    const agent = plugins.probe.agent.status();
    const condition: Context["agent"] =
      view.workspace.kind !== "project" || !agent.enabled
        ? { status: "off" }
        : agent.clients[0] !== undefined
          ? { status: "connected", client: agent.clients[0] }
          : { status: "on" };
    return { ...base, workspace: kinds[view.workspace.kind], agent: condition };
  }

  function show(path: string): WindowView {
    const id = nextId++;
    const workspace: WorkspaceView = /\.(md|markdown|mdown|txt)$/i.test(path)
      ? {
          kind: "singleFile",
          id,
          name: nameOf(path),
          path,
          folder: path.replace(/[\\/][^\\/]*$/, ""),
        }
      : { kind: "project", id, name: nameOf(path), root: path, stateLocation: "profile" };
    view = { state: "showing", workspace, document: null };
    if (workspace.kind === "project") shell.reset(path);
    if (workspace.kind === "project") {
      recent = [
        { path, name: workspace.name, openedMs: Date.now(), missing: false },
        ...recent.filter((r) => r.path !== path),
      ];
    }
    emit("window-changed", { window: "main", view });
    emit("context-changed", { window: "main", context: context() });
    return view;
  }

  return {
    emit,
    probe,
    appInfo: () => ok({ version: MOCK_VERSION }),
    windowBoot: () =>
      ok({
        window: "main",
        view,
        context: context(),
        settings,
        notices: [],
        failures: [],
        platform: "windows",
      }),
    openPath: (path, placement) => {
      if (placement === "ask" && view.state === "showing") return ok({ outcome: "needsChoice" });
      if (placement === "newWindow") return ok({ outcome: "openedElsewhere", window: "w-1" });
      return ok({ outcome: "opened", view: show(path) });
    },
    openNotes: (placement) => {
      if (placement === "ask" && view.state === "showing" && view.workspace.kind !== "notes") {
        return ok({ outcome: "needsChoice" });
      }
      if (placement === "newWindow") return ok({ outcome: "openedElsewhere", window: "w-1" });
      view = {
        state: "showing",
        workspace: { kind: "notes", id: nextId++, name: "Notes", folder: MOCK_NOTES_FOLDER },
        document: null,
      };
      emit("window-changed", { window: "main", view });
      emit("context-changed", { window: "main", context: context() });
      return ok({ outcome: "opened", view });
    },
    openInProject: (path) => {
      probe.opened.push({ path, mode: "project" });
      return ok(null);
    },
    showWelcome: () => {
      view = { state: "welcome" };
      tree = null;
      emit("window-changed", { window: "main", view });
      emit("context-changed", { window: "main", context: context() });
      return ok(null);
    },
    ...shell.api,
    pluginsList: () => shell.api.pluginsList(inProject()),
    projectInit: (path) => {
      const shown = shell.initProject(path);
      if (shown && view.state === "showing" && view.workspace.kind === "project") {
        view = { ...view, workspace: { ...view.workspace, stateLocation: "project" } };
        emit("window-changed", { window: "main", view });
        emit("context-changed", { window: "main", context: context() });
      }
      return ok(null);
    },
    recentList: () => ok(recent),
    recentRemove: (path) => {
      recent = recent.filter((r) => r.path !== path);
      return ok(null);
    },
    settingsGet: () => ok(settings),
    settingsSet: (change) => {
      settings = applyChange(settings, change);
      emit("settings-changed", { scope: { scope: "app" }, keys: [change.key], settings });
      if (change.key === "pluginEnabled") announceShell();
      return ok(settings);
    },
    projectSessionGet: (key) => ok(session.get(key) ?? null),
    projectSessionSet: (key, value) => {
      if (value === null) session.delete(key);
      else session.set(key, value);
      return ok(null);
    },
    treeOpen: (filter) => ok(snapshot(filter ?? DEFAULT_FILTER)),
    treeSetFilter: (filter) => ok(snapshot(filter)),
    treeOpenFolder: () => ok(null),
    treeKeep: (path) => {
      if (tree) tree = { ...tree, keep: path };
      return ok(null);
    },
    treeClose: () => {
      tree = null;
      return ok(null);
    },
    planMove: (moves, updateLinks) => {
      try {
        return ok(project.plan(moves, updateLinks));
      } catch (e) {
        return Promise.resolve(fail("fileops_plan_move", e));
      }
    },
    applyMove: (planId, updateLinks) => {
      try {
        const specs = project.moves(planId);
        const outcome = project.apply(planId, updateLinks);
        refreshTree(specs);
        return ok(outcome);
      } catch (e) {
        return Promise.resolve(fail("fileops_apply_move", e));
      }
    },
    deleteFiles: (paths) => {
      try {
        project.delete(paths);
        refreshTree();
        return ok({ deleted: paths });
      } catch (e) {
        return Promise.resolve(fail("fileops_delete", e));
      }
    },
    newEntry: (dir, name, kind) => {
      try {
        const path = project.create(dir, name, kind);
        refreshTree();
        return ok(path);
      } catch (e) {
        return Promise.resolve(fail("fileops_new", e));
      }
    },
    undoFileOp: () => {
      const done = project.undo();
      if (done) refreshTree();
      return ok(done ? { kind: "move", restored: 1, skipped: [] } : null);
    },
    absPath: (path) => ok(path === "" ? MOCK_FOLDER : `${MOCK_FOLDER}/${path}`),
    openExternal: (path, mode) => {
      probe.opened.push({ path, mode });
      return ok(null);
    },
    openDocument: (path, known) => {
      if (view.state === "showing" && view.workspace.kind === "notes") {
        const text = editing.probe.files.get(path);
        if (text === undefined) {
          return Promise.resolve({
            ok: false,
            error: {
              kind: "backend",
              command: "document_open",
              error: { kind: "notFound", subject: path, detail: null },
            },
          });
        }
        probe.shown = { path, known };
        return ok(mockNoteView(path, text));
      }
      // a single file shows its own name; the mock serves the showcase for it
      const asked =
        view.state === "showing" && view.workspace.kind === "singleFile" ? "README.md" : path;
      const found = mockView(asked, known, project.exists(path));
      probe.shown = { path, known };
      if (!found) {
        return Promise.resolve({
          ok: false,
          error: {
            kind: "backend",
            command: "document_open",
            error: { kind: "notFound", subject: path, detail: null },
          },
        });
      }
      return ok({ ...found, path, name: nameOf(path) });
    },
    closeDocument: () => {
      probe.shown = { path: null, known: [] };
      return ok(null);
    },
    documentText: (path) =>
      ok(
        view.state === "showing" && view.workspace.kind === "notes"
          ? (editing.probe.files.get(path) ?? "")
          : `# ${path}\n`,
      ),
    resolveLinks: (doc, hrefs) =>
      ok(hrefs.map((href) => mockLinkTarget(doc, href, (p) => project.exists(p)))),
    resolveCodeRefs: (_doc, written) =>
      ok(written.map((w) => (project.exists(w.replace(/^\//, "")) ? [w.replace(/^\//, "")] : []))),
    openInEditor: (path, line, col) => {
      probe.editor.push({ path, line, col });
      return ok(null);
    },
    openWeb: (url) => {
      probe.web.push(url);
      return ok(null);
    },
    ...editing.api,
    ...createMockConfig((path) => editing.probe.files.get(path) ?? null),
    searchStatus: () =>
      ok({
        phase: "ready",
        done: project.files.size,
        total: project.files.size,
        files: project.files.size,
        sections: project.files.size,
      }),
    searchSections: (query, limit, options) =>
      ok(mockSections(project, query, limit, organizing.allows(options))),
    searchFiles: (query, limit, folderLimit, options) =>
      ok(mockFiles(project, query, limit, folderLimit, organizing.allows(options))),
    searchRecent: (opened, limit, options) =>
      ok(mockRecent(project, opened, limit, organizing.allows(options))),
    assetUrl: (_workspace, path) => mockAssetUrl(path),
    showDocument: (path) => {
      if (view.state === "showing") {
        view = { ...view, document: path };
        emit("window-changed", { window: "main", view });
      }
      return ok(null);
    },
    on: (event, handler) => {
      listeners[event].add(handler);
      const unlisten: Unlisten = () => {
        listeners[event].delete(handler);
      };
      return Promise.resolve(unlisten);
    },
    onDrop: (handler) => {
      if (typeof window === "undefined") return Promise.resolve(() => {});
      const count = (e: DragEvent): number => e.dataTransfer?.items.length || 1;
      const over = (e: DragEvent): void => {
        e.preventDefault();
        handler({ type: "over", count: count(e) });
      };
      const leave = (e: DragEvent): void => {
        if (e.relatedTarget === null) handler({ type: "leave" });
      };
      const drop = (e: DragEvent): void => {
        e.preventDefault();
        handler({ type: "drop", paths: Array.from({ length: count(e) }, () => MOCK_FOLDER) });
      };
      window.addEventListener("dragover", over);
      window.addEventListener("dragleave", leave);
      window.addEventListener("drop", drop);
      return Promise.resolve(() => {
        window.removeEventListener("dragover", over);
        window.removeEventListener("dragleave", leave);
        window.removeEventListener("drop", drop);
      });
    },
    pickFolder: () => Promise.resolve(MOCK_FOLDER),
    pickFile: () => Promise.resolve(MOCK_FILE),
    setWindowTheme: (theme) => {
      windowThemes.push(theme);
      return Promise.resolve();
    },
    setTitle: (title) => {
      if (typeof document !== "undefined") document.title = title;
      return Promise.resolve();
    },
    showWindow: () => ok(undefined),
    pluginCall: plugins.call,
    defaultAppGet: distribution.defaultAppGet,
    defaultAppSet: distribution.defaultAppSet,
    defaultAppAsked: distribution.defaultAppAsked,
    defaultAppHandoff: distribution.defaultAppHandoff,
    onAppPlugin: (event, handler) => {
      appListeners[event].add(handler);
      return Promise.resolve(() => {
        appListeners[event].delete(handler);
      });
    },
    onPlugin: (event, handler) => {
      pluginListeners[event].add(handler);
      return Promise.resolve(() => {
        pluginListeners[event].delete(handler);
      });
    },
  };
}

export const mockApi = createMockApi();
