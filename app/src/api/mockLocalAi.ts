// What the local AI plugin answers in the mock backend: the catalog (the bundled models + mock-only fixtures, same
// shapes as the real plugin), the task list, installs (timer-driven downloads, terms, token, assignments). In memory.
// Fixtures cover every install state: installed, update available, too large for the mock disk, terms to accept,
// paused at 42 %, gated (token), failed (trying again fails its check), long German name with CJK languages (small).
// Store: default folder; a chosen one (refused inside | holding a mock project); move by timer steps (cancel, fail
// fixture `probe.failMove`) | keep (each folder keeps its own models). A chosen folder whose name has "unplugged" in
// it is gone (store unavailable) - the unavailable fixture for dev:mock.
// Machine: a 6 GiB laptop with a 2 GiB graphics card (`MOCK_MACHINE`): small models run well, mid ones slow, the
// largest not recommended; assigning a second big model (LFM3 beside Gemma 4 E2B) raises the budget warning.

import TASKS from "../plugins/local-ai/tasks.json";
import { accepts } from "../plugins/local-ai/tasks";
import type { AiTask } from "./generated/AiTask";
import type { AiTaskInfo } from "./generated/AiTaskInfo";
import type { CatalogModel } from "./generated/CatalogModel";
import type { DownloadError } from "./generated/DownloadError";
import type { GetBlock } from "./generated/GetBlock";
import type { InstallState } from "./generated/InstallState";
import type { LocalAiCatalog } from "./generated/LocalAiCatalog";
import type { LocalAiState } from "./generated/LocalAiState";
import type { MachineInfo } from "./generated/MachineInfo";
import type { MachineProfile } from "./generated/MachineProfile";
import type { ModelFit } from "./generated/ModelFit";
import type { ModelNeeds } from "./generated/ModelNeeds";
import type { ModelLicense } from "./generated/ModelLicense";
import type { ModelStatus } from "./generated/ModelStatus";
import type { StoreMode } from "./generated/StoreMode";
import type { StoreMove } from "./generated/StoreMove";
import { MockPluginError } from "./mockOrganizing";
import type { AppPluginEventName, AppPluginEvents, PluginMethods } from "./types";

type LocalAiMethods = Pick<
  PluginMethods,
  | "local-ai.catalog"
  | "local-ai.tasks"
  | "local-ai.state"
  | "local-ai.get"
  | "local-ai.update"
  | "local-ai.acceptLicense"
  | "local-ai.cancel"
  | "local-ai.deletePart"
  | "local-ai.remove"
  | "local-ai.assign"
  | "local-ai.setToken"
  | "local-ai.clearToken"
  | "local-ai.setStore"
  | "local-ai.cancelMove"
  | "local-ai.revealStore"
  | "local-ai.machine"
>;

const APACHE: ModelLicense = {
  name: "Apache 2.0",
  spdx: "Apache-2.0",
  url: "https://www.apache.org/licenses/LICENSE-2.0",
  restriction: null,
  needsAcceptance: false,
};
const MIB = 1_048_576;
const GIB = 1_073_741_824;

/** A catalog entry; `needs` follows the catalog's rule (files x 1.2 + context overhead). */
function model(
  fields: Pick<CatalogModel, "id" | "name" | "family" | "repo" | "bytes" | "capabilities"> &
    Partial<CatalogModel>,
): CatalogModel {
  const ramMin = Math.ceil((fields.bytes * 1.2) / MIB) * MIB;
  const overhead = fields.capabilities.includes("chat") ? GIB : GIB / 4;
  return {
    lineage: fields.id.replace(/-q\w+$/, ""),
    catalogVersion: 1,
    publisher: fields.repo.split("/")[0] ?? "",
    commit: "0".repeat(40),
    quant: "Q4_0",
    params: 0,
    contextLen: 8192,
    needs: { ramMin, ramRec: ramMin + overhead, vramRec: ramMin + overhead },
    license: APACHE,
    gated: false,
    languages: ["mul"],
    withdrawn: false,
    ...fields,
  };
}

/** The bundled `catalog.json` models (sizes = pinned totals) + mock-only fixtures (see the file comment). */
export const MOCK_CATALOG: LocalAiCatalog = {
  schemaVersion: 1,
  models: [
    model({
      id: "embeddinggemma-2-q8_0",
      name: "EmbeddingGemma 2",
      family: "EmbeddingGemma",
      repo: "ggml-org/embeddinggemma-2-GGUF",
      bytes: 309_855_456,
      capabilities: ["embed"],
      quant: "Q8_0",
    }),
    model({
      id: "gemma-4-e2b-it-q4_0",
      name: "Gemma 4 E2B",
      family: "Gemma 4",
      repo: "ggml-org/gemma-4-E2B-it-GGUF",
      bytes: 3_398_849_248,
      capabilities: ["chat", "tools", "transcribe", "vision"],
      contextLen: 131_072,
      // newer than the installed fixture version: update available
      catalogVersion: 2,
    }),
    model({
      id: "gemma-4-e4b-it-q4_0",
      name: "Gemma 4 E4B",
      family: "Gemma 4",
      repo: "ggml-org/gemma-4-E4B-it-GGUF",
      bytes: 5_150_682_208,
      capabilities: ["chat", "tools", "transcribe", "vision"],
      contextLen: 131_072,
    }),
    model({
      id: "lfm3-1.7b-q4_k_m",
      name: "LFM3 1.7B",
      family: "LFM3",
      repo: "LiquidAI/LFM3-1.7B-GGUF",
      bytes: 1_099_627_808,
      capabilities: ["chat", "tools"],
      quant: "Q4_K_M",
      contextLen: 262_144,
      license: {
        name: "LFM Open License v1.0",
        spdx: null,
        url: "https://www.liquid.ai/lfm-license",
        restriction: "Free for organizations under $10M annual revenue",
        needsAcceptance: true,
      },
      languages: ["en", "ar", "zh", "fr", "de", "it", "ja", "ko", "pt", "es", "vi", "th", "id"],
    }),
    model({
      id: "whisper-large-v3-turbo-q5_0",
      name: "Whisper large-v3-turbo",
      family: "Whisper",
      repo: "ggerganov/whisper.cpp",
      bytes: 574_041_195,
      capabilities: ["transcribe"],
      quant: "q5_0",
      contextLen: 448,
      license: {
        name: "MIT",
        spdx: "MIT",
        url: "https://opensource.org/license/mit",
        restriction: null,
        needsAcceptance: false,
      },
    }),
    // mock-only fixtures
    model({
      id: "gemma-3-270m-it-q8_0",
      name: "Gemma 3 270M",
      family: "Gemma 3",
      repo: "google/gemma-3-270m-it-GGUF",
      bytes: 241_410_624,
      capabilities: ["chat"],
      quant: "Q8_0",
      gated: true,
      license: {
        name: "Gemma Terms of Use",
        spdx: null,
        url: "https://ai.google.dev/gemma/terms",
        restriction: null,
        needsAcceptance: false,
      },
    }),
    model({
      id: "embeddinggemma-300m-q4_0",
      name: "EmbeddingGemma 300M",
      family: "EmbeddingGemma",
      repo: "ggml-org/embeddinggemma-300M-GGUF",
      bytes: 278_000_000,
      capabilities: ["embed"],
    }),
    model({
      id: "fachtext-uebersetzer-q4_0",
      name: "Übersetzungsmodell für wissenschaftliche Fachtexte und Fachwörterbücher (Deutsch, Japanisch, Chinesisch, Koreanisch)",
      family: "Fachtext",
      repo: "ggml-org/fachtext-uebersetzer-GGUF",
      bytes: 157_286_400,
      capabilities: ["translate"],
      languages: ["de", "ja", "zh-Hans", "zh-Hant", "ko", "en", "fr", "it"],
    }),
  ],
};

/** This computer in the mock: 6 GiB RAM, a 2 GiB card beside the integrated one, the software renderer. */
export const MOCK_MACHINE: MachineProfile = {
  ramTotal: 6 * GIB,
  ramAvailable: 2.5 * GIB,
  cpuLogical: 8,
  gpus: [
    {
      name: "Intel(R) Iris(R) Xe Graphics",
      dedicatedVram: 128 * MIB,
      shared: 3 * GIB,
      unified: false,
      software: false,
    },
    {
      name: "NVIDIA GeForce MX450",
      dedicatedVram: 2 * GIB,
      shared: 3 * GIB,
      unified: false,
      software: false,
    },
    {
      name: "Microsoft Basic Render Driver",
      dedicatedVram: 0,
      shared: 3 * GIB,
      unified: false,
      software: true,
    },
  ],
  os: "windows",
};

/** Largest memory of a real adapter (`null`: none known); as `MachineProfile::best_vram`. */
function bestVram(machine: MachineProfile): number | null {
  const known = machine.gpus
    .filter((g) => !g.software)
    .flatMap((g) => (g.dedicatedVram === null ? [] : [g.dedicatedVram]));
  return known.length === 0 ? null : Math.max(...known);
}

/** Fit hint, same rules as `fit.rs` (see its module doc). */
function fitOf(needs: ModelNeeds, machine: MachineProfile): ModelFit {
  if ((bestVram(machine) ?? -1) >= needs.vramRec) return "runsWell";
  if (machine.ramTotal === null) return "unknown";
  if (machine.ramTotal < needs.ramRec) return "notRecommended";
  return machine.gpus.some((g) => g.unified && !g.software) ? "runsWell" : "slowCpuOnly";
}

/** Free space of the mock disk: the largest model does not fit. */
export const MOCK_FREE_SPACE = 4 * GIB;
/** Room kept free beyond a download (as the real engine). */
const MARGIN = 64 * MIB;

export type LocalAiProbe = {
  /** Calls made, `method model`. */
  calls: string[];
  /** Time of one download step (each adds 10 % of the model). Read when a download starts. */
  stepMs: number;
  /** Free bytes on the mock disk. */
  freeSpace: number;
  /** The next download ends in this failure (`null`: per fixture). */
  failNext: DownloadError | null;
  /** The next store move ends in this failure (`null`: it succeeds). */
  failMove: DownloadError | null;
  /** This computer as `local-ai.machine` reads it. */
  machine: MachineProfile;
  /** Stops the download + move timers (tests). */
  stopAll(): void;
};

/** Default store of the mock profile. */
export const MOCK_DEFAULT_STORE =
  "C:\\Users\\mock\\AppData\\Roaming\\PerMark\\plugins\\local-ai\\models";

/** Path as compared: `/` separators, no trailing one, case folded (Windows-like mock). */
function keyOf(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

/** One folder is the other or inside it, either way. */
function overlap(a: string, b: string): boolean {
  const [x, y] = [keyOf(a), keyOf(b)];
  return x === y || x.startsWith(`${y}/`) || y.startsWith(`${x}/`);
}

/** In-memory activity of one model (absent = look at what is installed). */
type Live =
  | { kind: "queued"; done: number }
  | { kind: "running"; done: number }
  | { kind: "paused"; done: number }
  | { kind: "failed"; done: number; error: DownloadError };

const WHISPER = "whisper-large-v3-turbo-q5_0";
const FLAKY = "embeddinggemma-300m-q4_0";

export function createMockLocalAi(
  emit: <E extends AppPluginEventName>(event: E, payload: AppPluginEvents[E]) => void = () => {},
  /** Open + recent project folders (a store must stay clear of them). */
  projectFolders: () => string[] = () => [],
) {
  // the shared fixture is the Rust table (parity test); JSON import is typed structurally
  const tasks = TASKS.tasks as AiTaskInfo[];
  const models = MOCK_CATALOG.models;
  const byId = new Map(models.map((m) => [m.id, m]));
  /** Lineage -> installed version, of the store in use. */
  let installed = new Map<string, number>([
    ["embeddinggemma-2", 1],
    ["gemma-4-e2b-it", 1],
  ]);
  /** Store key -> what it holds (the one in use included). */
  const shelves = new Map<string, Map<string, number>>([[keyOf(MOCK_DEFAULT_STORE), installed]]);
  /** Chosen store (`null`: the default). */
  let chosen: string | null = null;
  let storeMove: StoreMove | null = null;
  let moveTimer: ReturnType<typeof setInterval> | null = null;
  const live = new Map<string, Live>([
    [WHISPER, { kind: "paused", done: Math.round(574_041_195 * 0.42) }],
    [FLAKY, { kind: "failed", done: 83_400_000, error: { kind: "offline" } }],
  ]);
  /** Fails its check when tried again. */
  const failsOnRetry = new Set<string>([FLAKY]);
  const assignments = new Map<AiTask, string>([
    ["semanticIndex", "embeddinggemma-2"],
    ["correction", "gemma-4-e2b-it"],
  ]);
  const accepted = new Set<string>();
  let tokenSet = false;
  /** Waiting downloads; the first one runs (one at a time, as the real engine). */
  const queue: string[] = [];
  let timer: ReturnType<typeof setInterval> | null = null;
  const probe: LocalAiProbe = {
    calls: [],
    stepMs: 400,
    freeSpace: MOCK_FREE_SPACE,
    failNext: null,
    failMove: null,
    machine: structuredClone(MOCK_MACHINE),
    stopAll: () => {
      if (timer !== null) clearInterval(timer);
      timer = null;
      stopMove();
    },
  };

  function stopMove(): void {
    if (moveTimer !== null) clearInterval(moveTimer);
    moveTimer = null;
  }

  function storePath(): string {
    return chosen ?? MOCK_DEFAULT_STORE;
  }

  function available(): boolean {
    return chosen === null || !keyOf(chosen).includes("unplugged");
  }

  function bytesOf(lineage: string): number {
    return models.find((m) => m.lineage === lineage)?.bytes ?? 0;
  }

  function modelOf(id: string): CatalogModel {
    const found = byId.get(id);
    if (found === undefined) throw new MockPluginError("notFound", id);
    return found;
  }

  function blockedOf(m: CatalogModel): GetBlock | null {
    if (m.withdrawn) return "withdrawn";
    if (m.gated && !tokenSet) return "token";
    if (m.license.needsAcceptance && !accepted.has(m.license.url)) return "license";
    return null;
  }

  function installState(m: CatalogModel): InstallState {
    const now = live.get(m.id);
    const total = m.bytes;
    if (now?.kind === "queued") return { kind: "queued" };
    if (now?.kind === "running")
      return now.done >= total
        ? { kind: "verifying", done: total, total }
        : { kind: "downloading", done: now.done, total };
    if (now?.kind === "paused") return { kind: "paused", done: now.done, total };
    if (now?.kind === "failed") return { kind: "failed", error: now.error };
    const version = installed.get(m.lineage);
    if (version === undefined)
      return available() ? { kind: "notInstalled" } : { kind: "storeUnavailable" };
    return version < m.catalogVersion
      ? { kind: "updateAvailable", installedVersion: version }
      : { kind: "installed", catalogVersion: version };
  }

  function status(m: CatalogModel): ModelStatus {
    return {
      model: m.id,
      state: installState(m),
      installedVersion: installed.get(m.lineage) ?? null,
      blocked: blockedOf(m),
    };
  }

  function state(): LocalAiState {
    return {
      models: models.map(status),
      assignments: tasks.map(({ task }) => {
        const lineage = assignments.get(task);
        const m = models.find((x) => x.lineage === lineage && installed.has(x.lineage));
        return { task, model: m?.id ?? null };
      }),
      store: {
        path: storePath(),
        isDefault: chosen === null,
        available: available(),
        freeBytes: available() ? probe.freeSpace : null,
        usedBytes: [...installed.keys()].reduce((sum, lineage) => sum + bytesOf(lineage), 0),
        partialBytes: [...live.values()].reduce((sum, now) => sum + now.done, 0),
      },
      storeMove: storeMove === null ? null : { ...storeMove },
      tokenSet,
      acceptedLicenses: [...accepted],
    };
  }

  function announce(): void {
    emit("local-ai:state", state());
  }

  function busy(id: string): boolean {
    const now = live.get(id)?.kind;
    return now === "queued" || now === "running";
  }

  /** One step of the running download (first of the queue): +10 %, then verifying, then done. */
  function step(): void {
    const id = queue[0];
    const now = id === undefined ? undefined : live.get(id);
    if (id === undefined || now === undefined) {
      probe.stopAll();
      return;
    }
    const m = modelOf(id);
    if (now.kind === "queued") {
      live.set(id, { kind: "running", done: now.done });
    } else if (now.kind === "running" && now.done < m.bytes) {
      now.done = Math.min(m.bytes, now.done + Math.ceil(m.bytes / 10));
    } else {
      finish(id, m);
      return;
    }
    emit("local-ai:progress", { model: id, state: installState(m) });
  }

  function finish(id: string, m: CatalogModel): void {
    queue.shift();
    const failure: DownloadError | null =
      probe.failNext ?? (failsOnRetry.has(id) ? { kind: "integrity" } : null);
    probe.failNext = null;
    if (failure !== null) {
      live.set(id, { kind: "failed", done: 0, error: failure });
    } else {
      live.delete(id);
      installed.set(m.lineage, m.catalogVersion);
    }
    if (queue.length === 0) probe.stopAll();
    announce();
  }

  function start(m: CatalogModel): void {
    const done = live.get(m.id)?.done ?? 0;
    const remaining = m.bytes - done;
    const needed = remaining + Math.max(MARGIN, Math.ceil(remaining / 100));
    if (needed > probe.freeSpace) {
      live.set(m.id, {
        kind: "failed",
        done,
        error: { kind: "noSpace", needed, free: probe.freeSpace },
      });
      announce();
      return;
    }
    live.set(m.id, { kind: "queued", done });
    queue.push(m.id);
    if (timer === null) timer = setInterval(step, probe.stepMs);
    announce();
  }

  /** Download of `id` stopped: the part kept (paused) or dropped. */
  function stopped(id: string, keep: boolean): void {
    const at = queue.indexOf(id);
    if (at >= 0) queue.splice(at, 1);
    const done = live.get(id)?.done ?? 0;
    if (keep && done > 0) live.set(id, { kind: "paused", done });
    else live.delete(id);
    if (queue.length === 0) probe.stopAll();
  }

  function notMoving(): void {
    if (storeMove?.kind === "moving") throw new MockPluginError("conflict", "store", "moving");
  }

  /** Uses `to` (`null`: the default) from now on; its own models listed, the old store's activity forgotten. */
  function switchTo(to: string | null): void {
    chosen = to;
    const key = keyOf(storePath());
    installed = shelves.get(key) ?? new Map<string, number>();
    shelves.set(key, installed);
    live.clear();
  }

  /** One move step: +10 %; done -> the new store in use (fails with `probe.failMove`). */
  function moveStep(to: string | null): void {
    if (storeMove?.kind !== "moving") {
      stopMove();
      return;
    }
    if (storeMove.done < storeMove.total) {
      storeMove.done = Math.min(storeMove.total, storeMove.done + Math.ceil(storeMove.total / 10));
      emit("local-ai:move", { ...storeMove });
      return;
    }
    stopMove();
    const failure = probe.failMove;
    probe.failMove = null;
    if (failure !== null) {
      storeMove = { kind: "failed", to: storeMove.to, error: failure };
    } else {
      const moved = installed;
      shelves.delete(keyOf(storePath()));
      storeMove = null;
      switchTo(to);
      for (const [lineage, version] of moved) installed.set(lineage, version);
    }
    announce();
  }

  function setStore(path: string | null, mode: StoreMode | undefined): LocalAiState {
    notMoving();
    if (path !== null && !/^([A-Za-z]:[\\/]|\/)/.test(path))
      throw new MockPluginError("invalidPath", path, "notAbsolute");
    const to = path === null || keyOf(path) === keyOf(MOCK_DEFAULT_STORE) ? null : path;
    if (keyOf(to ?? MOCK_DEFAULT_STORE) === keyOf(storePath())) return state();
    if (to !== null) {
      if (overlap(to, storePath()) || overlap(to, MOCK_DEFAULT_STORE))
        throw new MockPluginError("denied", to, "insideStore");
      if (projectFolders().some((project) => overlap(to, project)))
        throw new MockPluginError("denied", to, "insideProject");
    }
    const running = [...live].find(([, now]) => now.kind === "queued" || now.kind === "running");
    if (running !== undefined) throw new MockPluginError("conflict", running[0], "downloading");
    const target = to ?? MOCK_DEFAULT_STORE;
    const has = available() && installed.size > 0;
    if (has && mode === undefined) throw new MockPluginError("conflict", target, "modeRequired");
    if (!has || mode === "keep") {
      switchTo(to);
      storeMove = null;
    } else {
      const total = [...installed.keys()].reduce((sum, lineage) => sum + bytesOf(lineage), 0);
      storeMove = { kind: "moving", to: target, done: 0, total };
      stopMove();
      moveTimer = setInterval(() => moveStep(to), probe.stepMs);
    }
    announce();
    return state();
  }

  function get(id: string): LocalAiState {
    const m = modelOf(id);
    notMoving();
    if (!available()) throw new MockPluginError("unavailable", "store");
    const block = blockedOf(m);
    if (block !== null) throw new MockPluginError("denied", id, block);
    if (!busy(id) && installed.get(m.lineage) !== m.catalogVersion) start(m);
    return state();
  }

  /** Profile, fits, budget (`fit.rs` rules: assigned models' `ramRec` vs best VRAM + 60 % of RAM). */
  function machine(): MachineInfo {
    const profile = structuredClone(probe.machine);
    const fits = Object.fromEntries(models.map((m) => [m.id, fitOf(m.needs, profile)]));
    const assigned = new Set(
      state().assignments.flatMap((a) => (a.model === null ? [] : [a.model])),
    );
    const needed = [...assigned].reduce((sum, id) => sum + modelOf(id).needs.ramRec, 0);
    const room =
      profile.ramTotal === null
        ? null
        : (bestVram(profile) ?? 0) + Math.floor(profile.ramTotal / 5) * 3;
    const budget = room !== null && needed > room ? { needed, available: room } : null;
    return { profile, fits, budget };
  }

  const handlers: {
    [M in keyof LocalAiMethods]: (args: LocalAiMethods[M]["args"]) => LocalAiMethods[M]["result"];
  } = {
    "local-ai.catalog": () => structuredClone(MOCK_CATALOG),
    "local-ai.tasks": () => structuredClone(tasks),
    "local-ai.state": () => state(),
    "local-ai.get": ({ model }) => {
      probe.calls.push(`get ${model}`);
      return get(model);
    },
    "local-ai.update": ({ model }) => {
      probe.calls.push(`update ${model}`);
      const m = modelOf(model);
      const version = installed.get(m.lineage);
      if (version === undefined || version >= m.catalogVersion)
        throw new MockPluginError("conflict", model, "noUpdate");
      return get(model);
    },
    "local-ai.acceptLicense": ({ model }) => {
      probe.calls.push(`acceptLicense ${model}`);
      const m = modelOf(model);
      if (m.license.needsAcceptance) accepted.add(m.license.url);
      announce();
      return state();
    },
    "local-ai.cancel": ({ model }) => {
      probe.calls.push(`cancel ${model}`);
      modelOf(model);
      if (busy(model)) {
        stopped(model, true);
        announce();
      }
      return state();
    },
    "local-ai.deletePart": ({ model }) => {
      probe.calls.push(`deletePart ${model}`);
      modelOf(model);
      notMoving();
      stopped(model, false);
      announce();
      return state();
    },
    "local-ai.remove": ({ model }) => {
      probe.calls.push(`remove ${model}`);
      const m = modelOf(model);
      notMoving();
      if (busy(model)) throw new MockPluginError("conflict", model, "downloading");
      installed.delete(m.lineage);
      live.delete(model);
      for (const [task, lineage] of assignments)
        if (lineage === m.lineage) assignments.delete(task);
      announce();
      return state();
    },
    "local-ai.assign": ({ task, model }) => {
      probe.calls.push(`assign ${task} ${model ?? "-"}`);
      if (model === null) {
        assignments.delete(task);
      } else {
        const m = modelOf(model);
        if (!accepts(task, m.capabilities))
          throw new MockPluginError("unsupported", model, "incompatible");
        if (!installed.has(m.lineage)) throw new MockPluginError("conflict", model, "notInstalled");
        assignments.set(task, m.lineage);
      }
      announce();
      return state();
    },
    "local-ai.setToken": ({ token }) => {
      probe.calls.push("setToken");
      if (token.trim() === "") throw new MockPluginError("invalidData", "credentials", "empty");
      tokenSet = true;
      announce();
      return state();
    },
    "local-ai.clearToken": () => {
      probe.calls.push("clearToken");
      tokenSet = false;
      announce();
      return state();
    },
    "local-ai.setStore": ({ path, mode }) => {
      probe.calls.push(`setStore ${path ?? "-"} ${mode ?? "-"}`);
      return setStore(path, mode);
    },
    "local-ai.cancelMove": () => {
      probe.calls.push("cancelMove");
      if (storeMove?.kind === "moving") {
        stopMove();
        storeMove = null;
        announce();
      }
      return state();
    },
    "local-ai.revealStore": () => {
      probe.calls.push(`revealStore ${storePath()}`);
      if (!available()) throw new MockPluginError("unavailable", "store");
      return null;
    },
    "local-ai.machine": () => machine(),
  };
  /** Bytes "Remove data" frees (the plugins page asks while the plugin is off). */
  function dataBytes(): number {
    const now = state().store;
    return now.usedBytes + now.partialBytes;
  }
  return { handlers, probe, dataBytes };
}
