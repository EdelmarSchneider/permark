// Version source = app/package.json `version`. `sync` writes it everywhere else; `check` fails on drift (in the gate).
// Surgical: only the version text changes; every other byte (order, spacing, CRLF | LF, trailing newline) stays.
//
//   node scripts/version.mjs sync | check

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

/** @typedef {{ path: string, update: (text: string, version: string) => string }} Target */

const VERSION = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;

/**
 * Package version -> validated version string.
 * @param {string} packageJson
 * @returns {string}
 */
export function readVersion(packageJson) {
  /** @type {unknown} */
  const parsed = JSON.parse(packageJson);
  const version =
    typeof parsed === "object" && parsed !== null && "version" in parsed
      ? parsed.version
      : undefined;
  if (typeof version !== "string" || !VERSION.test(version)) {
    throw new Error(`app/package.json: version ${JSON.stringify(version)} is not x.y.z[-pre]`);
  }
  return version;
}

/**
 * `version = "..."` inside `[workspace.package]` of a Cargo.toml.
 * @param {string} text
 * @param {string} version
 * @returns {string}
 */
export function setCargoWorkspaceVersion(text, version) {
  const section = /^\[workspace\.package\][^\S\r\n]*$/m.exec(text);
  if (!section) throw new Error("no [workspace.package] section");
  const start = section.index + section[0].length;
  const next = /^\[/m.exec(text.slice(start));
  const end = next ? start + next.index : text.length;
  const body = text.slice(start, end);
  const line = /^(version\s*=\s*")([^"\r\n]*)(")/m;
  if (!line.test(body)) throw new Error("no version in [workspace.package]");
  return text.slice(0, start) + body.replace(line, `$1${version}$3`) + text.slice(end);
}

/**
 * Versions of the workspace packages (`permark`, `permark-*`, no `source`) in Cargo.lock.
 * @param {string} text
 * @param {string} version
 * @returns {string}
 */
export function setCargoLockVersions(text, version) {
  const entry =
    /(name = "permark(?:-[a-z0-9-]+)?"\r?\nversion = ")([^"\r\n]*)("\r?\n(?!source = ))/g;
  let count = 0;
  const out = text.replace(entry, (_whole, head, _old, tail) => {
    count += 1;
    return `${head}${version}${tail}`;
  });
  if (count === 0) throw new Error("no workspace package in Cargo.lock");
  return out;
}

/**
 * Every `"version": "..."` of a JSON file; their count + places must be exactly `paths` (anything else -> error).
 * @param {string} text
 * @param {string} version
 * @param {readonly (readonly (string | number)[])[]} paths
 * @returns {string}
 */
export function setJsonVersions(text, version, paths) {
  let count = 0;
  const out = text.replace(/("version"\s*:\s*")([^"\\\r\n]*)(")/g, (_whole, head, _old, tail) => {
    count += 1;
    return `${head}${version}${tail}`;
  });
  if (count !== paths.length) {
    throw new Error(`expected ${paths.length} "version" key(s), found ${count}`);
  }
  /** @type {unknown} */
  const parsed = JSON.parse(out);
  for (const path of paths) {
    /** @type {unknown} */
    let node = parsed;
    for (const key of path) {
      node = typeof node === "object" && node !== null ? Reflect.get(node, key) : undefined;
    }
    if (node !== version) throw new Error(`"version" not at ${path.join(".")}`);
  }
  return out;
}

/**
 * Replaces every match of `pattern` (global, group 1 = prefix, group 2 = suffix); at least one required.
 * @param {RegExp} pattern
 * @param {string} what
 * @returns {(text: string, version: string) => string}
 */
export function setByPattern(pattern, what) {
  return (text, version) => {
    let count = 0;
    const out = text.replace(pattern, (_whole, head, tail) => {
      count += 1;
      return `${head}${version}${tail}`;
    });
    if (count === 0) throw new Error(`no ${what}`);
    return out;
  };
}

/** @type {readonly Target[]} */
export const TARGETS = [
  { path: "app/src-tauri/Cargo.toml", update: setCargoWorkspaceVersion },
  { path: "app/src-tauri/Cargo.lock", update: setCargoLockVersions },
  { path: "app/src-tauri/tauri.conf.json", update: (t, v) => setJsonVersions(t, v, [["version"]]) },
  { path: ".claude-plugin/plugin.json", update: (t, v) => setJsonVersions(t, v, [["version"]]) },
  {
    path: ".claude-plugin/marketplace.json",
    update: (t, v) =>
      setJsonVersions(t, v, [
        ["metadata", "version"],
        ["plugins", 0, "version"],
      ]),
  },
  { path: ".cursor-plugin/plugin.json", update: (t, v) => setJsonVersions(t, v, [["version"]]) },
  { path: "gemini-extension.json", update: (t, v) => setJsonVersions(t, v, [["version"]]) },
  {
    path: "Dockerfile",
    update: setByPattern(/^(ARG FALLBACK_VERSION=)[^\r\n]*()$/gm, "ARG FALLBACK_VERSION"),
  },
  {
    path: "llms-install.md",
    update: setByPattern(/(PerMark_)[0-9A-Za-z.-]+(_amd64\.deb)/g, "PerMark_<version>_amd64.deb"),
  },
];

/**
 * Files whose content differs from what the version requires.
 * @param {string} root repo root
 * @param {string} version
 * @param {readonly Target[]} targets
 * @returns {{ path: string, text: string }[]}
 */
export function plan(root, version, targets = TARGETS) {
  /** @type {{ path: string, text: string }[]} */
  const changes = [];
  for (const target of targets) {
    const file = join(root, target.path);
    const before = readFileSync(file, "utf8");
    let after;
    try {
      after = target.update(before, version);
    } catch (error) {
      throw new Error(`${target.path}: ${error instanceof Error ? error.message : String(error)}`);
    }
    if (after !== before) changes.push({ path: target.path, text: after });
  }
  return changes;
}

/**
 * @param {readonly string[]} argv
 * @returns {number} exit code
 */
function main(argv) {
  const mode = argv[0];
  if (mode !== "sync" && mode !== "check") {
    console.error("usage: node scripts/version.mjs sync | check");
    return 2;
  }
  const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
  const version = readVersion(readFileSync(join(root, "app", "package.json"), "utf8"));
  const changes = plan(root, version);
  if (mode === "check") {
    if (changes.length === 0) {
      console.log(`version ${version}: in sync`);
      return 0;
    }
    console.error(`version ${version}: drift in ${changes.map((c) => c.path).join(", ")}`);
    console.error("fix: npm run version:sync");
    return 1;
  }
  for (const change of changes) writeFileSync(join(root, change.path), change.text);
  console.log(`version ${version}: ${changes.length} file(s) written`);
  return 0;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
