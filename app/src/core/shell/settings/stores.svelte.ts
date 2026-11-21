// What the settings page reads from the backend besides the app settings: the project's shared settings and the plugin
// list. Both reload on demand; a change goes to the backend and the answer is what shows.

import {
  api,
  type Api,
  type ApiError,
  type PluginInfo,
  type ProjectSettingChange,
  type ProjectSettings,
  type Sharing,
  type Unlisten,
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
  /** Why the last change of what git sees of `.permark/` was refused (cleared by the next one that works). */
  sharingError = $state.raw<ApiError | null>(null);
  #api: Api = api;
  #seq = 0;

  async load(backend: Api = api): Promise<void> {
    this.state = { status: "loading" };
    await this.#read(backend);
  }

  /** Reads the settings of the folder at `root` now and again whenever they change (a setting written in another
   *  window, the file changed on disk: a git pull, a hand edit); returns what stops following. Listens first, then
   *  reads: a change between the two is never lost. */
  follow(root: string | null, backend: Api = api): () => void {
    this.state = { status: "loading" };
    let gone = false;
    let off: Unlisten | null = null;
    void backend
      .on("project-config-changed", (e) => {
        if (!gone && e.root === root) void this.#read(backend);
      })
      .then((stop) => {
        if (gone) {
          stop();
          return;
        }
        off = stop;
        void this.#read(backend);
      });
    return () => {
      gone = true;
      off?.();
    };
  }

  /** What shows stays until the answer is there (read again after a change: no flash of an empty page). */
  async #read(backend: Api): Promise<void> {
    this.#api = backend;
    const seq = ++this.#seq;
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

  /** What git may see of `.permark/` (PerMark's own `.permark/.gitignore`). `true` when it was kept; else
   *  `sharingError` says why and the folder is read again (the file may hold someone else's text now). */
  async setSharing(sharing: Sharing): Promise<boolean> {
    const result = await this.#api.projectSharingSet(sharing);
    if (!result.ok) {
      this.sharingError = result.error;
      await this.load(this.#api);
      return false;
    }
    this.sharingError = null;
    this.state = { status: "ready", settings: result.value };
    return true;
  }

  clearProblem(): void {
    this.problem = null;
    this.sharingError = null;
  }

  /** The shared file sets plugin key `key` (`<id>.<key>`), which no plugin reads: the person's own value applies. */
  ignores(key: string): boolean {
    return this.state.status === "ready" && this.state.settings.ignoredPluginKeys.includes(key);
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

  /** Reads the list now and again whenever the plugins or the conditions of window `label` change (another window's
   *  conditions are not this list's); returns what stops following. */
  follow(label: string, backend: Api = api): () => void {
    void this.load(backend);
    const offs: (() => void)[] = [];
    let gone = false;
    const keep = (off: () => void): void => {
      if (gone) off();
      else offs.push(off);
    };
    void backend.on("plugins-changed", () => void this.load(backend)).then(keep);
    void backend
      .on("context-changed", (e) => {
        if (e.window === label) void this.load(backend);
      })
      .then(keep);
    return () => {
      gone = true;
      for (const off of offs) off();
    };
  }
}

export const pluginsStore = new PluginsStore();
