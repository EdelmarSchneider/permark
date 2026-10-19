// What the shell's backend half answers in the mock backend: the plugin list with its switches, the project's shared
// settings, the other workspaces of the process. In memory; same shapes as the real commands.

import type { Context } from "./generated/Context";
import type { Manifest } from "./generated/Manifest";
import type { PluginInfo } from "./generated/PluginInfo";
import type { PluginKind } from "./generated/PluginKind";
import type { ProjectSettingChange } from "./generated/ProjectSettingChange";
import type { ProjectSettings } from "./generated/ProjectSettings";
import type { WorkspaceSummary } from "./generated/WorkspaceSummary";
import type { AppSettings, Result } from "./types";

const PROJECT: Manifest["requires"] = {
  needs: [{ need: "workspace", kinds: ["project"] }],
  prefers: [],
};
const NONE: Manifest["requires"] = { needs: [], prefers: [] };

function manifest(
  id: string,
  kind: PluginKind,
  scope: Manifest["scope"],
  dependsOn: string[] = [],
): Manifest {
  return {
    id,
    kind,
    scope,
    requires: scope === "project" ? PROJECT : NONE,
    dependsOn,
    defaultEnabled: true,
  };
}

/** The plugins of the full build (same ids, kinds, scopes and dependencies as the Rust manifests). */
export const MOCK_MANIFESTS: readonly Manifest[] = [
  manifest("git", "feature", "project"),
  manifest("live-feed", "feature", "project"),
  manifest("follow", "feature", "project", ["live-feed"]),
  manifest("unread", "feature", "project"),
  manifest("review", "feature", "project", ["unread"]),
  manifest("tags", "feature", "project"),
  manifest("quiet", "feature", "project"),
  manifest("comments", "feature", "project"),
  manifest("broken-links", "checker", "project"),
  manifest("notes", "feature", "app"),
  manifest("agent", "feature", "project"),
  manifest("md-family", "role", "app"),
  manifest("word-count", "analysis", "app"),
  manifest("prompt-blocks", "embed", "app"),
  manifest("frontmatter-card", "embed", "app"),
  manifest("highlight", "embed", "app"),
  manifest("math", "embed", "app"),
  manifest("mermaid", "embed", "app"),
  manifest("plantuml", "embed", "app"),
  manifest("media", "format", "app"),
  manifest("json-view", "format", "app"),
  manifest("xml-view", "format", "app"),
  manifest("fmt-toml", "format", "app"),
  manifest("fmt-yaml", "format", "app"),
  manifest("fmt-ini", "format", "app"),
  manifest("fmt-dotenv", "format", "app"),
  manifest("fmt-properties", "format", "app"),
  manifest("fmt-ignore", "format", "app"),
  manifest("text-view", "format", "app"),
];

const ok = <T>(value: T): Promise<Result<T>> => Promise.resolve({ ok: true, value });

function refusal<T>(
  command: "project_config_set" | "plugin_remove_data" | "plugin_set_project",
  kind: "unavailable" | "invalidData" | "unsupported",
  subject: string,
  detail: string | null = null,
): Promise<Result<T>> {
  return Promise.resolve({
    ok: false,
    error: { kind: "backend", command, error: { kind, subject, detail } },
  });
}

function emptyProject(): ProjectSettings {
  return {
    initialized: false,
    tree: { show: null, include: [], exclude: [] },
    ignore: [],
    agentDeny: [],
    agentDenyPaths: [],
  };
}

/** Whether a list change is acceptable (the backend's limits, lighter). */
function listProblem(change: ProjectSettingChange): string | null {
  if (change.key === "treeShow") return null;
  const entries = change.value;
  if (entries.length > 200) return "tooMany";
  for (const entry of entries) {
    const text = entry.trim();
    if (text === "") return "empty";
    if (text.length > 300) return "tooLong";
    // eslint-disable-next-line no-control-regex
    if (/[\u0000-\u001f\u007f]/.test(text)) return "control";
    if (change.key === "agentDeny" && !/^[a-z]+(\.[a-z]+)?$/.test(text)) return "group";
  }
  return null;
}

export type MockShell = ReturnType<typeof createMockShell>;

/** `workspace`: what the window shows (so the list can name it); `changed`: tells the window the list moved. */
export function createMockShell(deps: {
  settings: () => AppSettings;
  workspace: () => WorkspaceSummary | null;
  changed: () => void;
}) {
  let project = emptyProject();
  /** The folders PerMark was initialized in (the real thing is a `.permark/` folder on disk). */
  const initializedPaths = new Set<string>();
  let currentPath = "";
  const choices = new Map<string, boolean>();
  const removed: string[] = [];
  let others: WorkspaceSummary[] = [];
  const focused: number[] = [];
  const closed: number[] = [];

  /** Plugins that are on for the window: settings, the project's own choices, dependencies. */
  function resolved(inProject: boolean): Map<string, Manifest> {
    const settings = deps.settings();
    const on = new Map<string, Manifest>();
    for (const m of MOCK_MANIFESTS) {
      let enabled = settings.plugins[m.id] ?? m.defaultEnabled;
      if (inProject && m.scope === "project" && choices.has(m.id))
        enabled = choices.get(m.id) ?? enabled;
      if (enabled && m.dependsOn.every((d) => on.has(d))) on.set(m.id, m);
    }
    return on;
  }

  return {
    /** Ids of the plugins on for the window now (the context's `plugins`). */
    pluginsOn(inProject: boolean): string[] {
      return [...resolved(inProject).keys()];
    },
    initialized: () => project.initialized,
    /** A project is shown: its settings are what its folder holds (nothing set, unless initialized before). */
    reset(path: string): void {
      currentPath = path;
      project = { ...emptyProject(), initialized: initializedPaths.has(path) };
      choices.clear();
    },
    context(base: Context): Context {
      return { ...base, permark: project.initialized ? "initialized" : "notInitialized" };
    },
    probe: {
      removed,
      focused,
      closed,
      /** The other workspaces of the process, as the backend lists them. */
      setOthers(list: WorkspaceSummary[]): void {
        others = list;
        deps.changed();
      },
      project: (): ProjectSettings => project,
      /** Every folder is as new: nothing is initialized. */
      forgetInit(): void {
        initializedPaths.clear();
        project = emptyProject();
      },
    },
    /** `.permark/` was created in `path`; `true` when that is the folder shown. */
    initProject(path: string): boolean {
      initializedPaths.add(path);
      if (path !== currentPath) return false;
      project = { ...project, initialized: true };
      return true;
    },
    api: {
      listWorkspaces: () => {
        const own = deps.workspace();
        return ok(own === null ? others : [own, ...others]);
      },
      focusWorkspace: (id: number) => {
        focused.push(id);
        return ok(null);
      },
      workspaceClose: (id: number) => {
        closed.push(id);
        return ok(null);
      },
      projectConfigGet: () => ok(project),
      projectConfigSet: (change: ProjectSettingChange) => {
        if (!project.initialized)
          return refusal<ProjectSettings>("project_config_set", "unavailable", "permark");
        const problem = listProblem(change);
        if (problem !== null) {
          return refusal<ProjectSettings>("project_config_set", "invalidData", change.key, problem);
        }
        const tidy = (v: string[]): string[] => [...new Set(v.map((s) => s.trim()))];
        switch (change.key) {
          case "treeShow":
            project = { ...project, tree: { ...project.tree, show: change.value } };
            break;
          case "treeInclude":
            project = { ...project, tree: { ...project.tree, include: tidy(change.value) } };
            break;
          case "treeExclude":
            project = { ...project, tree: { ...project.tree, exclude: tidy(change.value) } };
            break;
          case "ignore":
            project = { ...project, ignore: tidy(change.value) };
            break;
          case "agentDeny":
            project = { ...project, agentDeny: tidy(change.value) };
            break;
          case "agentDenyPaths":
            project = { ...project, agentDenyPaths: tidy(change.value) };
            break;
          default:
            change satisfies never;
        }
        return ok(project);
      },
      pluginsList: (inProject: boolean): Promise<Result<PluginInfo[]>> => {
        const settings = deps.settings();
        const on = resolved(inProject);
        const infos = MOCK_MANIFESTS.map((m): PluginInfo => {
          const appOn = settings.plugins[m.id] ?? m.defaultEnabled;
          const choice = inProject && m.scope === "project" ? (choices.get(m.id) ?? null) : null;
          const dependency = m.dependsOn.find((d) => !on.has(d));
          const state: PluginInfo["state"] = on.has(m.id)
            ? { state: "on" }
            : choice === false
              ? { state: "off", why: { reason: "project" } }
              : !(choice ?? appOn)
                ? { state: "off", why: { reason: "setting" } }
                : { state: "off", why: { reason: "dependency", plugin: dependency ?? "" } };
          return {
            manifest: m,
            appOn,
            state,
            projectChoice: choice,
            projectConfigOff: false,
          };
        });
        return ok(infos);
      },
      pluginSetProject: (plugin: string, enabled: boolean | null) => {
        const found = MOCK_MANIFESTS.find((m) => m.id === plugin);
        if (found === undefined || found.scope !== "project") {
          return refusal<null>("plugin_set_project", "unsupported", plugin);
        }
        if (enabled === null) choices.delete(plugin);
        else choices.set(plugin, enabled);
        deps.changed();
        return ok(null);
      },
      pluginRemoveData: (plugin: string) => {
        const found = MOCK_MANIFESTS.find((m) => m.id === plugin);
        if (found === undefined) return refusal<null>("plugin_remove_data", "unsupported", plugin);
        const settings = deps.settings();
        if (settings.plugins[plugin] ?? found.defaultEnabled) {
          return refusal<null>("plugin_remove_data", "unavailable", plugin);
        }
        removed.push(plugin);
        return ok(null);
      },
    },
  };
}
