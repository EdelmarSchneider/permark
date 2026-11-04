// What the local AI plugin answers in the mock backend: the catalog (the bundled models, same shapes as the real
// plugin) and the task list. In memory; states of installs come with the install handlers.

import TASKS from "../plugins/local-ai/tasks.json";
import type { AiTaskInfo } from "./generated/AiTaskInfo";
import type { CatalogModel } from "./generated/CatalogModel";
import type { LocalAiCatalog } from "./generated/LocalAiCatalog";
import type { ModelLicense } from "./generated/ModelLicense";
import type { PluginMethods } from "./types";

type LocalAiMethods = Pick<PluginMethods, "local-ai.catalog" | "local-ai.tasks">;

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

/** Same models as the bundled `catalog.json` (sizes = pinned totals). */
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
  ],
};

export function createMockLocalAi() {
  // the shared fixture is the Rust table (parity test); JSON import is typed structurally
  const tasks = TASKS.tasks as AiTaskInfo[];
  const handlers: {
    [M in keyof LocalAiMethods]: (args: LocalAiMethods[M]["args"]) => LocalAiMethods[M]["result"];
  } = {
    "local-ai.catalog": () => structuredClone(MOCK_CATALOG),
    "local-ai.tasks": () => structuredClone(tasks),
  };
  return { handlers };
}
