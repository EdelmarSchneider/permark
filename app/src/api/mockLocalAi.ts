// What the local AI plugin answers in the mock backend: the catalog (the bundled models + mock-only fixtures, same
// shapes as the real plugin), the task list, installs (timer-driven downloads, terms, token, assignments). In memory.
// Fixtures cover every install state: installed, update available, too large for the mock disk, terms to accept,
// paused at 42 %, gated (token), failed (trying again fails its check), long German name with CJK languages (small).

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
import type { ModelLicense } from "./generated/ModelLicense";
import type { ModelStatus } from "./generated/ModelStatus";
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
  /** Stops the download timer (tests). */
  stopAll(): void;
};

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
) {
  // the shared fixture is the Rust table (parity test); JSON import is typed structurally
  const tasks = TASKS.tasks as AiTaskInfo[];
  const models = MOCK_CATALOG.models;
  const byId = new Map(models.map((m) => [m.id, m]));
  /** Lineage -> installed version. */
  const installed = new Map<string, number>([
    ["embeddinggemma-2", 1],
    ["gemma-4-e2b-it", 1],
  ]);
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
    stopAll: () => {
      if (timer !== null) clearInterval(timer);
      timer = null;
    },
  };

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
    if (version === undefined) return { kind: "notInstalled" };
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
      store: "C:\\Users\\mock\\AppData\\Roaming\\PerMark\\plugins\\local-ai\\models",
      storeIsDefault: true,
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

  function get(id: string): LocalAiState {
    const m = modelOf(id);
    const block = blockedOf(m);
    if (block !== null) throw new MockPluginError("denied", id, block);
    if (!busy(id) && installed.get(m.lineage) !== m.catalogVersion) start(m);
    return state();
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
      stopped(model, false);
      announce();
      return state();
    },
    "local-ai.remove": ({ model }) => {
      probe.calls.push(`remove ${model}`);
      const m = modelOf(model);
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
  };
  return { handlers, probe };
}
