// Refreshes registry.json beside this script from the Agent Client Protocol registry. Developer tool, run by hand
// before a release: the app never fetches the registry, it embeds this snapshot at build time.
//
//   node app/src-tauri/crates/plugins/acp/agents/refresh.mjs
//
// The snapshot keeps only what the app reads (no icons, archives, checksums or authors). The registry data is
// licensed under the Apache License 2.0; each agent keeps its own license (field `license`). Entries the app must not
// start are left out with a warning: a package without one exact version, a variable that changes code loading or
// program lookup.

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

/** `x.y.z`, digits only: one exact version (no range, tag or `latest`). */
const EXACT = /^\d+\.\d+\.\d+$/;
/**
 * Variables that change how a runner loads code, finds programs, or where it downloads a pinned package from and how
 * it checks that download (a proxy, a registry | index, certificate checks).
 */
const LOADER_NAMES = new Set([
  "NODE_OPTIONS",
  "NODE_PATH",
  "PATH",
  "PATHEXT",
  "COMSPEC",
  "NODE_TLS_REJECT_UNAUTHORIZED",
  "NODE_EXTRA_CA_CERTS",
  "SSL_CERT_FILE",
  "SSL_CERT_DIR",
  "REQUESTS_CA_BUNDLE",
  "CURL_CA_BUNDLE",
]);
const LOADER_SUFFIXES = ["_PROXY"];
const LOADER_PREFIXES = ["LD_", "DYLD_", "PYTHON", "UV_", "PIP_", "NPM_CONFIG_", "BUN_"];

/** The version of a package spec (`npx`: `name@x.y.z`, a scoped name starts with `@`; `uvx`: also `name==x.y.z`). */
function versionOf(spec, uv) {
  if (uv && spec.includes("==")) return spec.slice(spec.indexOf("==") + 2);
  const at = spec.lastIndexOf("@");
  return at > 0 ? spec.slice(at + 1) : null;
}

function loaderVariable(name) {
  const upper = name.toUpperCase();
  return (
    LOADER_NAMES.has(upper) ||
    LOADER_SUFFIXES.some((suffix) => upper.endsWith(suffix)) ||
    LOADER_PREFIXES.some((prefix) => upper.startsWith(prefix))
  );
}

/**
 * Why the app must not start an agent as the registry describes it (`null`: fine): a package not pinned to one exact
 * version would run whatever is published later; a variable that changes loading or program lookup would let the
 * entry run other code. The Rust test `the_snapshot_pins_every_package_and_sets_no_loader_variable` guards the file.
 */
function refusal(agent) {
  const distribution = agent.distribution ?? {};
  for (const [runner, uv] of [
    ["npx", false],
    ["uvx", true],
  ]) {
    const spec = distribution[runner]?.package;
    if (spec !== undefined && !EXACT.test(versionOf(spec, uv) ?? "")) return `${runner} package not pinned: ${spec}`;
  }
  const envs = [distribution.npx?.env, distribution.uvx?.env, ...Object.values(distribution.binary ?? {}).map((b) => b.env)];
  const loader = envs.flatMap((env) => Object.keys(env ?? {})).find(loaderVariable);
  return loader === undefined ? null : `sets ${loader}`;
}

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

const accepted = registry.agents.filter((agent) => {
  const why = refusal(agent);
  if (why !== null) console.warn(`left out ${String(agent.id)}: ${why}`);
  return why === null;
});

const snapshot = {
  source: SOURCE,
  fetched: new Date().toISOString().slice(0, 10),
  license: "Apache-2.0",
  notice: NOTICE,
  agents: accepted.map((agent) => ({
    ...pick(agent, AGENT_FIELDS),
    distribution: trimDistribution(agent.distribution),
  })),
};

const target = join(dirname(fileURLToPath(import.meta.url)), "registry.json");
writeFileSync(target, `${JSON.stringify(snapshot, null, 2)}\n`);
console.log(`${snapshot.agents.length} agents -> ${target}`);
