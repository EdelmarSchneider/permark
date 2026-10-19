// Mock backend (`npm run dev:mock`, tests): same interface, no Tauri. One window ("main"), in memory.

import type { Context } from "./generated/Context";
import type { WindowView } from "./generated/WindowView";
import type { WorkspaceView } from "./generated/WorkspaceView";
import type {
  Api,
  AppSettings,
  EventName,
  Events,
  JsonValue,
  RecentFolder,
  Result,
  SettingChange,
  Unlisten,
} from "./types";

export const MOCK_VERSION = "0.0.0-mock";
export const MOCK_FOLDER = "C:/Users/demo/permark-docs";
export const MOCK_FILE = "C:/Users/demo/Downloads/proposal.md";

const ok = <T>(value: T): Promise<Result<T>> => Promise.resolve({ ok: true, value });

function defaultSettings(): AppSettings {
  return { language: "system", theme: "system", tone: "white", startIn: "welcome", plugins: {} };
}

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

/** Fresh mock (tests get their own; the dev build uses `mockApi`). */
export function createMockApi(): Api & {
  emit<E extends EventName>(event: E, payload: Events[E]): void;
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
  };

  function emit<E extends EventName>(event: E, payload: Events[E]): void {
    for (const handler of listeners[event]) handler(payload);
  }

  function context(): Context {
    const base = neutralContext();
    if (view.state !== "showing") return base;
    return { ...base, workspace: view.workspace.kind === "project" ? "project" : "singleFile" };
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
    appInfo: () => ok({ version: MOCK_VERSION }),
    windowBoot: () =>
      ok({ window: "main", view, context: context(), settings, notices: [], platform: "windows" }),
    openPath: (path, placement) => {
      if (placement === "ask" && view.state === "showing") return ok({ outcome: "needsChoice" });
      if (placement === "newWindow") return ok({ outcome: "openedElsewhere", window: "w-1" });
      return ok({ outcome: "opened", view: show(path) });
    },
    showWelcome: () => {
      view = { state: "welcome" };
      emit("window-changed", { window: "main", view });
      emit("context-changed", { window: "main", context: context() });
      return ok(null);
    },
    listWorkspaces: () => ok([]),
    focusWorkspace: () => ok(null),
    recentList: () => ok(recent),
    recentRemove: (path) => {
      recent = recent.filter((r) => r.path !== path);
      return ok(null);
    },
    settingsGet: () => ok(settings),
    settingsSet: (change) => {
      settings = applyChange(settings, change);
      emit("settings-changed", { scope: { scope: "app" }, keys: [change.key], settings });
      return ok(settings);
    },
    projectSessionGet: (key) => ok(session.get(key) ?? null),
    projectSessionSet: (key, value) => {
      if (value === null) session.delete(key);
      else session.set(key, value);
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
    setTitle: (title) => {
      if (typeof document !== "undefined") document.title = title;
      return Promise.resolve();
    },
    showWindow: () => ok(undefined),
  };
}

export const mockApi = createMockApi();
