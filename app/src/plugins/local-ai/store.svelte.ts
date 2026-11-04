// Local AI as the settings page sees it: the installs (state), the catalog (names, sizes, what each model can do), this
// computer (read once per page open, when a card needs it), and the person's actions. Nothing is read while the page
// is closed (the plugin is on but dormant); nothing here starts a download by itself: only `get` | `update` do.

import { api as defaultApi, type Api, type ApiError, type Result, type Unlisten } from "../../api";
import type { CatalogModel } from "../../api/generated/CatalogModel";
import type { InstallState } from "../../api/generated/InstallState";
import type { LocalAiCatalog } from "../../api/generated/LocalAiCatalog";
import type { LocalAiState } from "../../api/generated/LocalAiState";
import type { MachineInfo } from "../../api/generated/MachineInfo";
import type { ModelProgress } from "../../api/generated/ModelProgress";
import type { StoreMode } from "../../api/generated/StoreMode";
import type { StoreMove } from "../../api/generated/StoreMove";
import { pluginClient, type PluginClient } from "../../core/plugins/client";
import type { PluginPageId } from "../../core/shell/ui.svelte";
import { accepts, type AiTask } from "./tasks";

/** The settings page of the plugin. */
export const LOCAL_AI_PAGE: PluginPageId = "local-ai.page";

/** The page's first read. */
export type Load =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready" }
  | { status: "failed"; error: ApiError };

/** A download moment the page announces: start, each quarter, done, failure (never every tick). */
export type Milestone = { model: string; step: "started" | 25 | 50 | 75 | "done" | "failed" };

const RUNNING: ReadonlySet<InstallState["kind"]> = new Set(["queued", "downloading", "verifying"]);
const QUARTERS = [null, 25, 50, 75] as const;

/** Quarter a download has reached (0..3; `null`: not downloading). */
function quarter(state: InstallState): number | null {
  if (state.kind !== "downloading" || state.total <= 0) return null;
  return Math.min(3, Math.floor((state.done / state.total) * 4));
}

/** What one model's change from `before` to `after` tells the person (`null`: nothing worth saying). */
export function milestoneOf(before: InstallState, after: InstallState): Milestone["step"] | null {
  const was = RUNNING.has(before.kind);
  if (after.kind === "failed") return before.kind === "failed" ? null : "failed";
  if (after.kind === "installed") return was ? "done" : null;
  if (!RUNNING.has(after.kind)) return null;
  if (!was) return "started";
  const [from, to] = [quarter(before), quarter(after)];
  return from !== null && to !== null && to > from ? (QUARTERS[to] ?? null) : null;
}

/** Assignments in one string: a change asks this computer's budget again. */
function assignmentKey(state: LocalAiState): string {
  return state.assignments.map((a) => `${a.task}=${a.model ?? ""}`).join(",");
}

export class LocalAiStore {
  /** Installs, assignments, store, token (`null` until first read). */
  state = $state.raw<LocalAiState | null>(null);
  catalog = $state.raw<LocalAiCatalog | null>(null);
  /** This computer, fits, budget warning (`null` until asked by a card). */
  machine = $state.raw<MachineInfo | null>(null);
  load = $state.raw<Load>({ status: "idle" });
  /** Latest download milestone (the Models card's live region). */
  milestone = $state.raw<Milestone | null>(null);
  readonly #api: Api;
  readonly #client: PluginClient;
  /** Pages open (one per window in practice). */
  #opened = 0;
  /** Bumped on every listen | unlisten: a late answer of an earlier one is dropped. */
  #generation = 0;
  #listening: Unlisten[] = [];
  #machineAsked = false;

  constructor(api: Api = defaultApi, client: PluginClient = pluginClient) {
    this.#api = api;
    this.#client = client;
  }

  /** The page opens: reads the state + catalog and follows the backend's news; returns what the page calls on close. */
  open(): () => void {
    this.#opened += 1;
    if (this.#opened === 1) {
      this.#listen();
      void this.#read();
    }
    let closed = false;
    return () => {
      if (closed || this.#opened === 0) return;
      closed = true;
      this.#opened -= 1;
      if (this.#opened === 0) {
        this.#unlisten();
        this.#machineAsked = false;
      }
    };
  }

  /** The plugin is switched off: stops listening, forgets everything. */
  stop(): void {
    this.#opened = 0;
    this.#unlisten();
    this.#machineAsked = false;
    this.state = null;
    this.catalog = null;
    this.machine = null;
    this.milestone = null;
    this.load = { status: "idle" };
  }

  /** Reads again after a failure. */
  retry(): Promise<void> {
    return this.#read();
  }

  /** A card shows this computer | the budget: asks once per page open. */
  needMachine(): void {
    if (this.#machineAsked) return;
    this.#machineAsked = true;
    void this.#readMachine();
  }

  modelOf(id: string): CatalogModel | null {
    return this.catalog?.models.find((m) => m.id === id) ?? null;
  }

  /** Model chosen for `task` (`null`: not assigned). */
  assigned(task: AiTask): string | null {
    return this.state?.assignments.find((a) => a.task === task)?.model ?? null;
  }

  /** Models on this computer that can do `task`, catalog order. */
  choicesFor(task: AiTask): CatalogModel[] {
    const models = this.state?.models ?? [];
    return models.flatMap((status) => {
      const model = this.modelOf(status.model);
      return status.installedVersion !== null && model !== null && accepts(task, model.capabilities)
        ? [model]
        : [];
    });
  }

  // ---- actions: each answers the new state (applied here) or why it was refused

  async get(model: string): Promise<Result<LocalAiState>> {
    return this.#took(await this.#client.call("local-ai.get", { model }));
  }

  async update(model: string): Promise<Result<LocalAiState>> {
    return this.#took(await this.#client.call("local-ai.update", { model }));
  }

  async acceptLicense(model: string): Promise<Result<LocalAiState>> {
    return this.#took(await this.#client.call("local-ai.acceptLicense", { model }));
  }

  async cancel(model: string): Promise<Result<LocalAiState>> {
    return this.#took(await this.#client.call("local-ai.cancel", { model }));
  }

  async deletePart(model: string): Promise<Result<LocalAiState>> {
    return this.#took(await this.#client.call("local-ai.deletePart", { model }));
  }

  async remove(model: string): Promise<Result<LocalAiState>> {
    return this.#took(await this.#client.call("local-ai.remove", { model }));
  }

  async assign(task: AiTask, model: string | null): Promise<Result<LocalAiState>> {
    return this.#took(await this.#client.call("local-ai.assign", { task, model }));
  }

  async setToken(token: string): Promise<Result<LocalAiState>> {
    return this.#took(await this.#client.call("local-ai.setToken", { token }));
  }

  async clearToken(): Promise<Result<LocalAiState>> {
    return this.#took(await this.#client.call("local-ai.clearToken", {}));
  }

  /** `path: null` = back to the default folder; `mode` when installed models are there (move | keep). */
  async setStore(path: string | null, mode?: StoreMode): Promise<Result<LocalAiState>> {
    const args = mode === undefined ? { path } : { path, mode };
    return this.#took(await this.#client.call("local-ai.setStore", args));
  }

  async cancelMove(): Promise<Result<LocalAiState>> {
    return this.#took(await this.#client.call("local-ai.cancelMove", {}));
  }

  revealStore(): Promise<Result<null>> {
    return this.#client.call("local-ai.revealStore", {});
  }

  // ---- inside

  #took(result: Result<LocalAiState>): Result<LocalAiState> {
    if (result.ok) this.#apply(result.value);
    return result;
  }

  #apply(state: LocalAiState): void {
    const before = this.state;
    this.#note(before, state);
    this.state = state;
    // the budget follows the assignments (the backend reckons it)
    if (
      this.machine !== null &&
      (before === null || assignmentKey(before) !== assignmentKey(state))
    )
      void this.#readMachine();
  }

  #progress(progress: ModelProgress): void {
    const state = this.state;
    if (state === null) return;
    const next = {
      ...state,
      models: state.models.map((m) =>
        m.model === progress.model ? { ...m, state: progress.state } : m,
      ),
    };
    this.#note(state, next);
    this.state = next;
  }

  /** Keeps the last milestone between two states (the first read announces nothing). */
  #note(before: LocalAiState | null, after: LocalAiState): void {
    if (before === null) return;
    for (const now of after.models) {
      const was = before.models.find((m) => m.model === now.model);
      const step = was === undefined ? null : milestoneOf(was.state, now.state);
      if (step !== null) this.milestone = { model: now.model, step };
    }
  }

  #moving(move: Extract<StoreMove, { kind: "moving" }>): void {
    if (this.state !== null) this.state = { ...this.state, storeMove: move };
  }

  async #read(): Promise<void> {
    const generation = this.#generation;
    if (this.state === null) this.load = { status: "loading" };
    const [state, catalog] = await Promise.all([
      this.#client.call("local-ai.state", {}),
      this.#client.call("local-ai.catalog", {}),
    ]);
    if (generation !== this.#generation) return;
    if (!state.ok) {
      this.load = { status: "failed", error: state.error };
      return;
    }
    if (!catalog.ok) {
      this.load = { status: "failed", error: catalog.error };
      return;
    }
    this.catalog = catalog.value;
    this.#apply(state.value);
    this.load = { status: "ready" };
  }

  async #readMachine(): Promise<void> {
    const generation = this.#generation;
    const result = await this.#client.call("local-ai.machine", {});
    if (generation === this.#generation && result.ok) this.machine = result.value;
  }

  #listen(): void {
    this.#generation += 1;
    const generation = this.#generation;
    const keep = (listening: Promise<Unlisten>): void => {
      void listening.then(
        (stop) => {
          if (generation === this.#generation) this.#listening.push(stop);
          else stop();
        },
        () => {
          // no backend to listen to (a test, a browser): the reads are all there is
        },
      );
    };
    keep(this.#api.onAppPlugin("local-ai:state", (state) => this.#apply(state)));
    keep(this.#api.onAppPlugin("local-ai:progress", (progress) => this.#progress(progress)));
    keep(this.#api.onAppPlugin("local-ai:move", (move) => this.#moving(move)));
  }

  #unlisten(): void {
    this.#generation += 1;
    for (const stop of this.#listening.splice(0)) stop();
  }
}

/** The store of this window. */
export const localAi = new LocalAiStore();
