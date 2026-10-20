// What the settings page reads from the backend besides the app settings: the project's shared settings and the plugin
// list. Both reload on demand; a change goes to the backend and the answer is what shows.

import {
  api,
  type Api,
  type ApiError,
  type PluginInfo,
  type ProjectSettingChange,
  type ProjectSettings,
} from "../../../api";

export type ProjectState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; settings: ProjectSettings }
  | { status: "failed"; error: ApiError };

/** The reason code of a refused value (`tooMany`, `empty`, ...), or `notInitialized` | `other`. */
export function problemOf(error: ApiError): string {
  if (error.kind !== "backend") return "other";
  if (error.error.kind === "unavailable") return "notInitialized";
  return error.error.detail ?? "other";
}

/** The shared settings of the project this window shows (`.permark/config.toml`). */
class ProjectSettingsStore {
  state = $state.raw<ProjectState>({ status: "idle" });
  /** The last refused change: which key, and why (cleared by the next change that works). */
  problem = $state.raw<{ key: string; code: string } | null>(null);
  #api: Api = api;
  #seq = 0;

  async load(backend: Api = api): Promise<void> {
    this.#api = backend;
    const seq = ++this.#seq;
    this.state = { status: "loading" };
    const result = await backend.projectConfigGet();
    if (seq !== this.#seq) return;
    this.state = result.ok
      ? { status: "ready", settings: result.value }
      : { status: "failed", error: result.error };
  }

  /** One setting. `true` when it was kept; else `problem` says why. */
  async set(change: ProjectSettingChange): Promise<boolean> {
    const result = await this.#api.projectConfigSet(change);
    if (!result.ok) {
      this.problem = { key: change.key, code: problemOf(result.error) };
      return false;
    }
    this.problem = null;
    this.state = { status: "ready", settings: result.value };
    return true;
  }

  clearProblem(): void {
    this.problem = null;
  }
}

export const projectSettings = new ProjectSettingsStore();

export type PluginsState =
  | { status: "loading" }
  | { status: "ready"; plugins: readonly PluginInfo[] }
  | { status: "failed"; error: ApiError };

/** The plugins with their state for this window. */
class PluginsStore {
  state = $state.raw<PluginsState>({ status: "loading" });
  #seq = 0;

  async load(backend: Api = api): Promise<void> {
    const seq = ++this.#seq;
    const result = await backend.pluginsList();
    if (seq !== this.#seq) return;
    this.state = result.ok
      ? { status: "ready", plugins: result.value }
      : { status: "failed", error: result.error };
  }
}

export const pluginsStore = new PluginsStore();
