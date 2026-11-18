// Refreshes registry.json beside this script from the Agent Client Protocol registry. Developer tool, run by hand
// before a release: the app never fetches the registry, it embeds this snapshot at build time.
//
//   node app/src-tauri/crates/plugins/acp/agents/refresh.mjs
//
// The snapshot keeps only what the app reads (no icons, archives, checksums or authors). The registry data is
// licensed under the Apache License 2.0; each agent keeps its own license (field `license`).

import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SOURCE = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
const NOTICE =
  "Agent list taken from the Agent Client Protocol registry (https://github.com/agentclientprotocol/registry), " +
  "licensed under the Apache License 2.0. Trimmed to the fields PerMark reads; each agent keeps its own license.";
const AGENT_FIELDS = ["id", "name", "version", "description", "website", "repository", "license"];
const PACKAGE_FIELDS = ["package", "args", "env"];
const BINARY_FIELDS = ["cmd", "args", "env"];

/** The listed `keys` of `value` that are present, in that order. */
function pick(value, keys) {
  return Object.fromEntries(keys.filter((key) => value?.[key] !== undefined).map((key) => [key, value[key]]));
}

function trimDistribution(distribution = {}) {
  const out = {};
  if (distribution.npx) out.npx = pick(distribution.npx, PACKAGE_FIELDS);
  if (distribution.uvx) out.uvx = pick(distribution.uvx, PACKAGE_FIELDS);
  if (distribution.binary) {
    out.binary = Object.fromEntries(
      Object.entries(distribution.binary).map(([platform, build]) => [platform, pick(build, BINARY_FIELDS)]),
    );
  }
  return out;
}

const response = await fetch(SOURCE);
if (!response.ok) throw new Error(`${SOURCE}: HTTP ${response.status}`);
const registry = await response.json();
if (!Array.isArray(registry.agents)) throw new Error(`${SOURCE}: no agents list`);

const snapshot = {
  source: SOURCE,
  fetched: new Date().toISOString().slice(0, 10),
  license: "Apache-2.0",
  notice: NOTICE,
  agents: registry.agents.map((agent) => ({
    ...pick(agent, AGENT_FIELDS),
    distribution: trimDistribution(agent.distribution),
  })),
};

const target = join(dirname(fileURLToPath(import.meta.url)), "registry.json");
writeFileSync(target, `${JSON.stringify(snapshot, null, 2)}\n`);
console.log(`${snapshot.agents.length} agents -> ${target}`);
