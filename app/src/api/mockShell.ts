// What the shell's backend half answers in the mock backend: the plugin list with its switches, the project's shared
// settings, where a folder's data is kept, the other windows of the process. In memory; same shapes as the real
// commands.

import type { Context } from "./generated/Context";
import type { DataOffer } from "./generated/DataOffer";
import type { Manifest } from "./generated/Manifest";
import type { PluginInfo } from "./generated/PluginInfo";
import type { PluginKind } from "./generated/PluginKind";
import type { ProjectSettingChange } from "./generated/ProjectSettingChange";
import type { ProjectSettings } from "./generated/ProjectSettings";
import type { WindowSummary } from "./generated/WindowSummary";
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
  defaultEnabled = true,
): Manifest {
  return {
    id,
    kind,
    scope,
    requires: scope === "project" ? PROJECT : NONE,
    dependsOn,
    defaultEnabled,
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
  // experimental: it starts the person's own program, so it waits until the person turns it on
  manifest("acp", "feature", "project", [], false),
  manifest("md-family", "role", "app"),
  manifest("symbols", "feature", "app"),
  manifest("word-count", "analysis", "app"),
  manifest("token-count", "analysis", "app"),
  manifest("local-ai", "feature", "app"),
  manifest("correction", "feature", "app", ["local-ai"]),
  manifest("translation", "feature", "app", ["local-ai"]),
  manifest("spelling", "feature", "app"),
  manifest("prompt-blocks", "embed", "app"),
  manifest("frontmatter-card", "embed", "app"),
  manifest("highlight", "embed", "app"),
  manifest("math", "embed", "app"),
  manifest("mermaid", "embed", "app"),
  manifest("plantuml", "embed", "app"),
  manifest("graphviz", "embed", "app"),
  manifest("media", "format", "app"),
  manifest("json-view", "format", "app"),
  manifest("xml-view", "format", "app"),
  manifest("fmt-toml", "format", "app"),
  manifest("fmt-yaml", "format", "app"),
  manifest("fmt-ini", "format", "app"),
  manifest("fmt-dotenv", "format", "app"),
  manifest("fmt-properties", "format", "app"),
  manifest("fmt-ignore", "format", "app"),
  manifest("fmt-toon", "format", "app"),
  manifest("convert", "feature", "app", ["fmt-toon"]),
  manifest("export", "feature", "app"),
  manifest("text-view", "format", "app"),
  {
    ...manifest("shell-integration", "feature", "app"),
    requires: {
      needs: [
        { need: "platform", platforms: ["windows"] },
        { need: "distribution", distribution: "direct" },
      ],
      prefers: [],
    },
  },
  {
    ...manifest("updates", "feature", "app"),
    requires: { needs: [{ need: "distribution", distribution: "direct" }], prefers: [] },
  },
];

const ok = <T>(value: T): Promise<Result<T>> => Promise.resolve({ ok: true, value });

function refusal<T>(
  command: "project_config_set" | "plugin_remove_data" | "plugin_data_bytes" | "plugin_set_project",
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
    tokenizers: [],
    ignoredPluginKeys: [],
  };
}

/** Whether a list change is acceptable (the backend's limits, lighter). */
function listProblem(change: ProjectSettingChange): string | null {
  if (change.key === "treeShow" || change.key === "pluginOff") return null;
  if (change.key === "tokenizers") {
    const ids = change.value;
    const valid = ids.every(
      (id, at) => /^[a-z0-9]{1,16}$/.test(id) && !ids.slice(0, at).includes(id),
    );
    return ids.length <= 3 && valid ? null : "tokenizers";
  }
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

/** Only a plugin of this build that runs per project can be turned on | off for one project. */
function isProjectPlugin(id: string): boolean {
  return MOCK_MANIFESTS.find((m) => m.id === id)?.scope === "project";
}

export type MockShell = ReturnType<typeof createMockShell>;

/** `changed`: tells the window the plugins | the window list moved. */
export function createMockShell(deps: {
  settings: () => AppSettings;
  changed: () => void;
  /** Bytes a plugin's "Remove data" frees (`null`: nothing to tell). */
  dataBytes?: (plugin: string) => number | null;
}) {
  let project = emptyProject();
  /** The folders PerMark was initialized in (the real thing is a `.permark/` folder on disk). */
  const initializedPaths = new Set<string>();
  /** What a folder's window offers about the place of its data (none listed: nothing). */
  const offers = new Map<string, DataOffer>();
  let currentPath = "";
  const choices = new Map<string, boolean>();
  /** Plugins the project's shared file turns off for everyone (`enabled = false`). */
  const teamOff = new Set<string>();
  const removed: string[] = [];
  let others: WindowSummary[] = [];
  const focused: number[] = [];

  /** Plugins that are on for the window: settings, the project's own choices, dependencies. */
  function resolved(inProject: boolean): Map<string, Manifest> {
    const settings = deps.settings();
    const on = new Map<string, Manifest>();
    for (const m of MOCK_MANIFESTS) {
      let enabled = settings.plugins[m.id] ?? m.defaultEnabled;
      if (inProject && m.scope === "project" && choices.has(m.id))
        enabled = choices.get(m.id) ?? enabled;
      if (inProject && m.scope === "project" && teamOff.has(m.id)) enabled = false;
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
      teamOff.clear();
    },
    context(base: Context): Context {
      return { ...base, permark: project.initialized ? "initialized" : "notInitialized" };
    },
    /** The other windows of the process, as the backend lists them. */
    others: (): WindowSummary[] => others,
    probe: {
      removed,
      focused,
      /** The other windows of the process (their tabs), as the backend lists them. */
      setOthers(list: WindowSummary[]): void {
        others = list;
        deps.changed();
      },
      project: (): ProjectSettings => project,
      /** The project's shared file holds plugin keys no plugin reads (`<id>.<key>`). */
      ignorePluginKeys(keys: string[]): void {
        project = { ...project, ignoredPluginKeys: keys };
      },
      /** Every folder is as new: nothing is initialized. */
      forgetInit(): void {
        initializedPaths.clear();
        offers.clear();
        project = emptyProject();
      },
      /** The folder at `path` offers `offer` about its data when it opens; with Move | Keep and both places holding
       *  data it has a team folder (a `.permark/` that arrived while the data stayed on this computer). */
      dataOffer(path: string, offer: DataOffer): void {
        offers.set(path, offer);
        if (offer === "ask" || offer === "movable" || offer === "both") initializedPaths.add(path);
        if (path === currentPath) project = { ...project, initialized: initializedPaths.has(path) };
      },
    },
    /** What the window of the folder at `path` offers about the place of its data. */
    offerOf: (path: string): DataOffer => offers.get(path) ?? "none",
    /** `.permark/` was created in `path` (the data moved in: nothing left to offer); `true` when that is the folder
     *  shown. */
    initProject(path: string): boolean {
      initializedPaths.add(path);
      offers.delete(path);
      if (path !== currentPath) return false;
      project = { ...project, initialized: true };
      return true;
    },
    /** The person keeps the data of the folder at `path` on this computer: Move alone is left after Move | Keep; the
     *  copy in use is kept as the place; `true` when that is the folder shown. */
    keepData(path: string): boolean {
      const offer = offers.get(path);
      if (offer === "ask") offers.set(path, "movable");
      if (offer === "missing") offers.delete(path);
      return path === currentPath;
    },
    api: {
      focusWorkspace: (id: number) => {
        focused.push(id);
        return ok(null);
      },
      projectConfigGet: () => ok(project),
      projectConfigSet: (change: ProjectSettingChange) => {
        if (change.key === "pluginOff" && !isProjectPlugin(change.value.plugin)) {
          return refusal<ProjectSettings>("project_config_set", "unsupported", change.value.plugin);
        }
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
          case "tokenizers":
            project = { ...project, tokenizers: change.value };
            break;
          case "pluginOff":
            if (change.value.off) teamOff.add(change.value.plugin);
            else teamOff.delete(change.value.plugin);
            deps.changed();
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
          const configOff = inProject && m.scope === "project" && teamOff.has(m.id);
          const state: PluginInfo["state"] = on.has(m.id)
            ? { state: "on" }
            : choice === false || configOff
              ? { state: "off", why: { reason: "project" } }
              : !(choice ?? appOn)
                ? { state: "off", why: { reason: "setting" } }
                : { state: "off", why: { reason: "dependency", plugin: dependency ?? "" } };
          return {
            manifest: m,
            appOn,
            state,
            projectChoice: choice,
            projectConfigOff: configOff,
          };
        });
        return ok(infos);
      },
      pluginSetProject: (plugin: string, enabled: boolean | null) => {
        if (!isProjectPlugin(plugin)) {
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
      pluginDataBytes: (plugin: string) => {
        if (!MOCK_MANIFESTS.some((m) => m.id === plugin))
          return refusal<number | null>("plugin_data_bytes", "unsupported", plugin);
        return ok(removed.includes(plugin) ? null : (deps.dataBytes?.(plugin) ?? null));
      },
    },
  };
}
