// Mock backend (`npm run dev:mock`, tests): same interface, no Tauri. One window ("main"), in memory; the page is the
// view of its active tab (switching tabs shows that tab's workspace in it; the app switches between views instead).

import { DEFAULT_FILTER } from "../core/explorer/filter";
import { opensAsText } from "../core/explorer/importModel";
import type { Context } from "./generated/Context";
import type { ErrorKind } from "./generated/ErrorKind";
import type { WindowView } from "./generated/WindowView";
import type { WorkspaceView } from "./generated/WorkspaceView";
import {
  MOCK_DOCUMENT_PATHS,
  mockAssetUrl,
  mockLinkTarget,
  mockNoteView,
  mockView,
} from "./mockDocs";
import { createMockConfig } from "./mockConfig";
import { createMockDistribution, type DistributionProbe } from "./mockDistribution";
import { createMockTokens, type TokensProbe } from "./mockTokens";
import { createMockConvert, type ConvertProbe } from "./mockConvert";
import { createMockEditing, type MockEditProbe } from "./mockEdit";
import { createMockImport, type ImportProbe } from "./mockImport";
import { createMockNotes, MOCK_NOTES_FOLDER } from "./mockNotes";
import { createMockOrganizing } from "./mockOrganizing";
import { createMockPlugins, type MockPluginProbe } from "./mockPlugins";
import { MockProject, MockRefusal, seedFiles } from "./mockFs";
import { createMockProjects, type ProjectsProbe } from "./mockProjects";
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
  TabSummary,
  Unlisten,
  WindowSummary,
  DropEvent,
  DropOpened,
  DropScope,
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
    openIn: "tab",
    gitBackend: "automatic",
    notesFolder: "",
    tokenizers: [],
    referenceTemplate: "{path}#{section}",
    editor: "",
    keymap: {},
    plugins: {},
    wrapLines: true,
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
    case "tokenizers":
      return { ...settings, tokenizers: change.value };
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
    case "wrapLines":
      return { ...settings, wrapLines: change.value };
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
  /** Folders the folder picker was asked to open at, in order (`null`: the system's choice). */
  pickedFrom: (string | null)[];
  /** The default app, the file manager entries, the updater. */
  distribution: DistributionProbe;
  /** The token count plugin: vocabularies, what it was asked to count. */
  tokens: TokensProbe;
  /** The converter: the next answer, calls, saved files. */
  convert: ConvertProbe;
  /** Drop into project: what lies outside, the plans and applies, a hold to keep one running. */
  imports: ImportProbe;
  /** Picture files read for a drop on an editor, by path. */
  pictures: string[];
  /** Drop to open: what was asked, paths that are gone (they fail), paths that are folders (else files, text by
   * name), and `fire` = a system drag event on the view. */
  drops: {
    opened: { paths: string[]; scope: DropScope }[];
    missing: Set<string>;
    folders: Set<string>;
    focused: number;
    fire: (event: DropEvent) => void;
  };
  /** The tabs: moved to a new window, moved into another window (`[tab, window]`), the close prompt's answers, saves
   * reported. `refuseMove`: the next moves fail (the backend could not move the view). */
  tabs: {
    moved: number[];
    movedTo: [number, string][];
    answers: string[];
    saved: boolean[];
    refuseMove: boolean;
  };
  /** Web pictures asked for, by address; the per-project choice. */
  remote: { asked: string[]; always: boolean };
  /** Projects inside the folder: the fixture per folder, what was asked, news for the Changes panel. */
  projects: ProjectsProbe;
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
    "windows-changed": new Set(),
    "settings-changed": new Set(),
    "open-failed": new Set(),
    "tree-changed": new Set(),
    "document-changed": new Set(),
    "close-requested": new Set(),
    "tab-close-requested": new Set(),
    "save-requested": new Set(),
    "view-moved": new Set(),
    "import-progress": new Set(),
    "plugins-changed": new Set(),
    "index-progress": new Set(),
    "projects-changed": new Set(),
  };
  const pluginListeners: { [E in PluginEventName]: Set<(payload: PluginEnvelope<E>) => void> } = {
    "git:snapshot": new Set(),
    "git:repos": new Set(),
    "git:projectSnapshot": new Set(),
    "live-feed:items": new Set(),
    "live-feed:activity": new Set(),
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

  type Shown = Extract<WindowView, { state: "showing" }>;
  /** The tabs of the window: what each shows (the page shows the active one's). */
  let tabList: { tab: number; view: Shown }[] = [];
  let activeTab: number | null = null;
  let nextTab = 1;

  function pathOfView(shown: Shown): string {
    const workspace = shown.workspace;
    switch (workspace.kind) {
      case "project":
        return workspace.root;
      case "singleFile":
        return workspace.path;
      case "notes":
        return "notes";
      default:
        return workspace satisfies never;
    }
  }

  function summaryOf(entry: { tab: number; view: Shown }): TabSummary {
    const workspace = entry.view.workspace;
    return {
      tab: entry.tab,
      kind: workspace.kind,
      title: workspace.kind === "notes" ? "Notes" : workspace.name,
      path: workspace.kind === "notes" ? "" : pathOfView(entry.view),
      loaded: true,
      unsaved: entry.tab === activeTab && editing.probe.unsaved !== null,
    };
  }

  function windowsNow(): WindowSummary[] {
    const own: WindowSummary = {
      window: "main",
      closing: false,
      tabs: tabList.map(summaryOf),
      active: activeTab,
      focusedMs: 0,
    };
    return [own, ...shell.others()];
  }

  function announceTabs(): void {
    emit("windows-changed", { windows: windowsNow() });
  }

  /** The page shows `next` (what the active tab shows), as the backend tells a view. */
  function showView(next: WindowView): void {
    view = next;
    const active = tabList.find((t) => t.tab === activeTab);
    if (active && next.state === "showing") active.view = next;
    emit("window-changed", {
      window: "main",
      view,
      tab: view.state === "welcome" ? null : activeTab,
    });
    emit("context-changed", { window: "main", context: context() });
  }

  function activateTab(tab: number): void {
    const entry = tabList.find((t) => t.tab === tab);
    if (!entry || tab === activeTab) return;
    activeTab = tab;
    tree = null;
    if (entry.view.workspace.kind === "project") shell.reset(entry.view.workspace.root);
    showView(entry.view);
    announceTabs();
  }

  function closeTab(tab: number): void {
    const at = tabList.findIndex((t) => t.tab === tab);
    if (at < 0) return;
    tabList = tabList.filter((t) => t.tab !== tab);
    if (activeTab === tab) {
      const next = tabList[Math.min(at, tabList.length - 1)];
      if (next) {
        activateTab(next.tab);
        return;
      }
      activeTab = null;
      tree = null;
      showView({ state: "welcome" });
    }
    announceTabs();
  }

  /** A tab already showing `path` (one place). */
  function tabOf(path: string): number | null {
    return tabList.find((t) => pathOfView(t.view) === path)?.tab ?? null;
  }

  /** The window list or the plugins moved: windows hear it, as from the backend. */
  function announceShell(): void {
    announceTabs();
    emit("plugins-changed", null);
    emit("context-changed", { window: "main", context: context() });
  }

  const shell = createMockShell({ settings: () => settings, changed: announceShell });
  const windowThemes: ("light" | "dark" | null)[] = [];
  const pickedFrom: (string | null)[] = [];

  const project = new MockProject();
  // read when asked (the projects mock is made below)
  const organizing = createMockOrganizing(emitPlugin, project, () => ({
    view: projects.probe.viewOf(rootNow()),
    root: rootNow(),
  }));
  const editing = createMockEditing(() => Object.keys(seedFiles()).sort());
  const notes = createMockNotes(editing.probe.files, () => {
    for (const handler of appListeners["notes:intent"]) handler(null);
  });
  const appListeners: { [E in AppPluginEventName]: Set<(payload: AppPluginEvents[E]) => void> } = {
    "updates:state": new Set(),
    "token-count:state": new Set(),
    "token-count:folder": new Set(),
    "token-count:folders": new Set(),
    "notes:intent": new Set(),
  };
  const distribution = createMockDistribution((event, payload) => {
    for (const handler of appListeners[event]) handler(payload);
  });
  const tokens = createMockTokens(
    (event, payload) => {
      for (const handler of appListeners[event]) handler(payload);
    },
    () => project.files,
    rootNow,
  );
  const convert = createMockConvert((path) => editing.probe.files.get(path) ?? null);
  const plugins = createMockPlugins(
    emitPlugin,
    () => mockView("docs/guide.md", [], true)?.hash ?? "0",
    organizing,
    notes,
    { ...distribution.handlers, ...tokens.handlers, ...convert.handlers },
    // read when asked (the projects mock is made just below)
    () => projects.probe.viewOf(rootNow()),
  );
  const importing = createMockImport(project, emit, () => refreshTree());
  const projects = createMockProjects({
    root: rootNow,
    tagsChanged: () => emitPlugin("tags:changed", null),
    emit,
    feed: (item) => emitPlugin("live-feed:items", { upsert: [item] }),
    addFiles: (files) => {
      for (const [path, text] of Object.entries(files)) {
        if (!project.files.has(path)) project.add(path, text);
      }
      refreshTree();
    },
  });
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
    pickedFrom,
    distribution: distribution.probe,
    tokens: tokens.probe,
    convert: convert.probe,
    imports: importing.probe,
    pictures: [],
    drops: {
      opened: [],
      missing: new Set(),
      folders: new Set(),
      focused: 0,
      fire: (event) => {
        for (const handler of dropHandlers) handler(event);
      },
    },
    remote: { asked: [], always: false },
    tabs: { moved: [], movedTo: [], answers: [], saved: [], refuseMove: false },
    projects: projects.probe,
  };
  let tree: TreeView | null = null;
  /** The view's drop listeners (the system's drag events, fired by tests through `probe.drops.fire`). */
  const dropHandlers = new Set<(event: DropEvent) => void>();

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
      codeHidden: counts.codeHidden,
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

  /** Opens `path` in a new tab (`this`: in the active one) and shows it. */
  /** A tab for `path` listed after the others, not shown (a pending tab). */
  function listTab(path: string): void {
    const shown = shownOf(path);
    tabList = [...tabList, { tab: nextTab++, view: shown }];
    announceTabs();
  }

  function shownOf(path: string): Shown {
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
    return { state: "showing", workspace, document: null };
  }

  function show(path: string, where: "tab" | "this" = "tab"): WindowView {
    const shown = shownOf(path);
    const workspace = shown.workspace;
    if (where === "tab" || activeTab === null) {
      activeTab = nextTab++;
      tabList = [...tabList, { tab: activeTab, view: shown }];
    }
    tree = null;
    if (workspace.kind === "project") shell.reset(path);
    if (workspace.kind === "project") {
      recent = [
        { path, name: workspace.name, openedMs: Date.now(), missing: false },
        ...recent.filter((r) => r.path !== path),
      ];
    }
    showView(shown);
    announceTabs();
    return view;
  }

  return {
    emit,
    probe,
    appInfo: () => ok({ version: MOCK_VERSION }),
    windowBoot: () =>
      ok({
        window: "main",
        label: "main",
        tab: activeTab,
        view,
        context: context(),
        settings,
        notices: [],
        failures: [],
        platform: "windows",
      }),
    openPath: (path, placement) => {
      const open = tabOf(path);
      if (open !== null) {
        activateTab(open);
        return ok({ outcome: "focused", window: "main", tab: open });
      }
      if (placement === "newWindow") return ok({ outcome: "openedElsewhere", window: "w-1" });
      // this tab (the mock's page shows what it opens either way: a first tab when there is none)
      if (placement === "thisTab") return ok({ outcome: "opened", view: show(path, "this") });
      show(path, "tab");
      return ok({ outcome: "newTab", tab: activeTab ?? 0 });
    },
    openNotes: (placement) => {
      const open = tabOf("notes");
      if (open !== null) {
        activateTab(open);
        return ok({ outcome: "focused", window: "main", tab: open });
      }
      if (placement === "newWindow") return ok({ outcome: "openedElsewhere", window: "w-1" });
      const shown: Shown = {
        state: "showing",
        workspace: { kind: "notes", id: nextId++, name: "Notes", folder: MOCK_NOTES_FOLDER },
        document: null,
      };
      const here = placement === "thisTab";
      if (!here || activeTab === null) {
        activeTab = nextTab++;
        tabList = [...tabList, { tab: activeTab, view: shown }];
      }
      tree = null;
      showView(shown);
      announceTabs();
      return here
        ? ok({ outcome: "opened", view })
        : ok({ outcome: "newTab", tab: activeTab ?? 0 });
    },
    openInProject: (path) => {
      probe.opened.push({ path, mode: "project" });
      return ok(null);
    },
    listWindows: () => ok(windowsNow()),
    tabActivate: (tab) => {
      activateTab(tab);
      return ok(null);
    },
    openDropped: (paths, scope) => {
      probe.drops.opened.push({ paths: [...paths], scope });
      const failed: DropOpened["failed"] = [];
      const left: DropOpened["left"] = [];
      let last: number | null = null;
      let added: string | null = null;
      for (const path of paths) {
        if (probe.drops.missing.has(path)) {
          failed.push({ path, error: { kind: "notFound", subject: path, detail: null } });
          continue;
        }
        const open = tabOf(path);
        if (open !== null) {
          if (added !== null) listTab(added);
          last = open;
          added = null;
          continue;
        }
        if (scope === "existingOnly") {
          left.push({ path, text: !probe.drops.folders.has(path) && opensAsText(path) });
          continue;
        }
        // listed now, shown only when it is the last one (a pending tab in the app)
        if (added !== null) listTab(added);
        added = path;
        last = null;
      }
      if (added !== null) show(added);
      else if (last !== null) activateTab(last);
      return ok({ failed, left });
    },
    focusView: () => {
      probe.drops.focused += 1;
      return ok(null);
    },
    tabClose: (tab) => {
      // unsaved edits in it: it comes forward and its view is asked first
      if (editing.probe.unsaved !== null && tab === activeTab) {
        emit("tab-close-requested", { window: "main" });
        return ok(null);
      }
      closeTab(tab);
      return ok(null);
    },
    tabCloseConfirmed: () => {
      if (activeTab !== null) closeTab(activeTab);
      return ok(null);
    },
    tabMoveNewWindow: (tab) => {
      if (probe.tabs.refuseMove) {
        return Promise.resolve(
          fail<string>("tab_move_new_window", new MockRefusal("unavailable", "main")),
        );
      }
      probe.tabs.moved.push(tab);
      closeTab(tab);
      return ok("w-1");
    },
    tabMove: (tab, window) => {
      if (probe.tabs.refuseMove) {
        return Promise.resolve(fail<string>("tab_move", new MockRefusal("unavailable", "main")));
      }
      probe.tabs.movedTo.push([tab, window]);
      closeTab(tab);
      return ok(window);
    },
    windowCloseAnswer: (answer) => {
      probe.tabs.answers.push(answer);
      if (answer === "saveAll") emit("save-requested", { window: "main" });
      else if (answer === "discard") editing.probe.closed = true;
      return ok(null);
    },
    viewSaved: (saved) => {
      probe.tabs.saved.push(saved);
      if (saved) editing.probe.closed = true;
      return ok(null);
    },
    ...shell.api,
    pluginsList: () => shell.api.pluginsList(inProject()),
    projectInit: (path) => {
      const shown = shell.initProject(path);
      if (shown && view.state === "showing" && view.workspace.kind === "project") {
        showView({ ...view, workspace: { ...view.workspace, stateLocation: "project" } });
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
    undoFileOpEntry: (id) => {
      const done = project.undoEntry(id);
      if (done) refreshTree();
      return ok(done ? { kind: "import", restored: 1, skipped: [] } : null);
    },
    ...importing.api,
    remotePicture: (url) => {
      probe.remote.asked.push(url);
      if (!url.startsWith("https://")) {
        return Promise.resolve({
          ok: false,
          error: {
            kind: "backend",
            command: "remote_picture",
            error: { kind: "unsupported", subject: null, detail: "https" },
          },
        });
      }
      return ok(new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
    },
    remotePicturesGet: () => ok(probe.remote.always),
    remotePicturesSet: (always) => {
      probe.remote.always = always;
      return ok(null);
    },
    readPicture: (path) => {
      probe.pictures.push(path);
      return ok(new Uint8Array([0x89, 0x50, 0x4e, 0x47]));
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
      // a single file shows its own name; the mock serves the showcase for it (the files next to it: the project's)
      const asked =
        view.state === "showing" &&
        view.workspace.kind === "singleFile" &&
        path === view.workspace.name
          ? "README.md"
          : path;
      // a Markdown file the probe | a test added (no rendered fixture): the simple view of its text
      const added = project.files.get(asked);
      const found =
        added !== undefined && /\.md$/i.test(asked) && !MOCK_DOCUMENT_PATHS.includes(asked)
          ? mockNoteView(asked, added, { inline: true })
          : mockView(asked, known, project.exists(path));
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
    documentMore: () => ok({ text: "", next: null }),
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
      // as the backend: no editor chosen, nothing starts
      if (settings.editor.trim() === "") {
        return Promise.resolve({
          ok: false,
          error: {
            kind: "backend",
            command: "open_in_editor",
            error: { kind: "editorNotSet", subject: null, detail: null },
          },
        });
      }
      probe.editor.push({ path, line, col });
      return ok(null);
    },
    openWeb: (url) => {
      probe.web.push(url);
      return ok(null);
    },
    ...editing.api,
    windowClose: () => {
      const unsaved = editing.probe.unsaved;
      const active = tabList.find((t) => t.tab === activeTab);
      if (unsaved !== null && active) {
        const tab = summaryOf(active);
        emit("close-requested", {
          window: "main",
          tabs: [{ tab: tab.tab, title: tab.title, document: unsaved }],
        });
        return ok(null);
      }
      editing.probe.closed = true;
      return ok(null);
    },
    ...createMockConfig((path) => editing.probe.files.get(path) ?? null),
    ...projects.handlers,
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
      ok(
        mockFiles(
          project,
          query,
          limit,
          folderLimit,
          organizing.allows(options),
          options.hiddenLast,
        ),
      ),
    searchRecent: (opened, limit, options) =>
      ok(mockRecent(project, opened, limit, organizing.allows(options))),
    assetUrl: (_workspace, path) => mockAssetUrl(path),
    showDocument: (path) => {
      if (view.state === "showing") showView({ ...view, document: path });
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
      dropHandlers.add(handler);
      if (typeof window === "undefined") {
        return Promise.resolve(() => {
          dropHandlers.delete(handler);
        });
      }
      const count = (e: DragEvent): number => e.dataTransfer?.items.length || 1;
      const over = (e: DragEvent): void => {
        e.preventDefault();
        // the browser hides what is dragged until the drop: the mock names the demo folder, once per item
        const paths = Array.from({ length: count(e) }, () => MOCK_FOLDER);
        handler({ type: "over", count: paths.length, paths, x: e.clientX, y: e.clientY });
      };
      const leave = (e: DragEvent): void => {
        if (e.relatedTarget === null) handler({ type: "leave" });
      };
      const drop = (e: DragEvent): void => {
        e.preventDefault();
        handler({
          type: "drop",
          paths: Array.from({ length: count(e) }, () => MOCK_FOLDER),
          x: e.clientX,
          y: e.clientY,
        });
      };
      window.addEventListener("dragover", over);
      window.addEventListener("dragleave", leave);
      window.addEventListener("drop", drop);
      return Promise.resolve(() => {
        dropHandlers.delete(handler);
        window.removeEventListener("dragover", over);
        window.removeEventListener("dragleave", leave);
        window.removeEventListener("drop", drop);
      });
    },
    pickFolder: (_title, from) => {
      pickedFrom.push(from ?? null);
      return Promise.resolve(MOCK_FOLDER);
    },
    pickFile: () => Promise.resolve(MOCK_FILE),
    pickPictures: () => Promise.resolve([`${MOCK_FOLDER}/pictures/diagram.png`]),
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
