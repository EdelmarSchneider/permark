// THIRD-PARTY-NOTICES.md: the licenses of everything third-party that ships inside PerMark, made at build time (release
// jobs + the CI `notices` job), shipped as a resource of every installer and opened from the app's About.
//
//   node scripts/third-party-notices.mjs [--out <file>] [--modules <node_modules dir>]
//
// Needs `cargo-about` on PATH (scripts/get-cargo-about.sh in CI; config src-tauri/about.toml + about.hbs) and `npm ci`
// done. Fails (no file written) when a Rust crate's license is not accepted in about.toml, when `cargo-about` or `cargo`
// are missing, or when a bundled npm package says nothing about its license.
//
// Three sources:
// - Rust crates (what is compiled into the program): `cargo about generate`, per license text with the crates using it.
// - npm packages (what the interface bundles): the dependency tree of `dependencies` in package.json + BUNDLED_DEV
//   (from package-lock.json), each one's license files read from node_modules, grouped by text the same way. A superset
//   of what the bundler keeps: over-listing is harmless, a miss is not.
// - libgit2: the C library compiled in by the `libgit2-sys` crate; cargo-about only sees the crate, so its COPYING (the
//   license + the notices of the components it bundles) is added as it is.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

/**
 * devDependencies whose code ends up in the interface bundle (the Svelte runtime). Everything else under
 * devDependencies is tooling.
 */
export const BUNDLED_DEV = ["svelte"];

/** @typedef {{ dependencies?: Record<string, string>, optionalDependencies?: Record<string, string>,
 *   peerDependencies?: Record<string, string>, optional?: boolean, version?: string }} LockEntry */
/** @typedef {{ packages: Record<string, LockEntry> }} Lock */
/** @typedef {{ name: string, version: string, license: string, text: string }} Notice */
/** @typedef {{ licenses: string[], text: string, users: string[] }} Group */

/**
 * Where `name` resolves to when required from the package at `from` ("" = the app): Node's walk up the node_modules
 * folders, on the lockfile's keys.
 * @param {Record<string, LockEntry>} packages
 * @param {string} from
 * @param {string} name
 * @returns {string | null}
 */
export function resolveDependency(packages, from, name) {
  let base = from;
  for (;;) {
    const key = `${base ? `${base}/` : ""}node_modules/${name}`;
    if (key in packages) return key;
    if (base === "") return null;
    const cut = base.lastIndexOf("/node_modules/");
    base = cut === -1 ? "" : base.slice(0, cut);
  }
}

/**
 * The lockfile keys of every package the interface can bundle: the roots (named dependencies of the app) and all that
 * they need, optional + peer ones that are installed too. Sorted.
 * @param {Lock} lock
 * @param {readonly string[]} roots
 * @returns {string[]}
 */
export function bundledKeys(lock, roots) {
  const { packages } = lock;
  /** @type {Set<string>} */
  const seen = new Set();
  /** @type {string[]} */
  const queue = [];
  for (const name of roots) {
    const key = resolveDependency(packages, "", name);
    if (key === null) throw new Error(`${name} is not in package-lock.json (run npm ci)`);
    queue.push(key);
  }
  for (let key = queue.pop(); key !== undefined; key = queue.pop()) {
    if (seen.has(key)) continue;
    seen.add(key);
    const entry = packages[key];
    if (!entry) continue;
    const needs = {
      ...entry.peerDependencies,
      ...entry.optionalDependencies,
      ...entry.dependencies,
    };
    for (const name of Object.keys(needs)) {
      const found = resolveDependency(packages, key, name);
      if (found !== null) queue.push(found);
    }
  }
  return [...seen].sort();
}

/**
 * Package name of a lockfile key ("node_modules/a/node_modules/@s/b" -> "@s/b").
 * @param {string} key
 * @returns {string}
 */
export function nameOfKey(key) {
  const cut = key.lastIndexOf("node_modules/");
  return key.slice(cut + "node_modules/".length);
}

/**
 * The license of a package.json as one SPDX-like string ("" when it names none).
 * @param {unknown} manifest
 * @returns {string}
 */
export function licenseOf(manifest) {
  if (typeof manifest !== "object" || manifest === null) return "";
  const m = /** @type {Record<string, unknown>} */ (manifest);
  const one = (/** @type {unknown} */ value) => {
    if (typeof value === "string") return value;
    if (typeof value === "object" && value !== null) {
      const type = /** @type {Record<string, unknown>} */ (value)["type"];
      if (typeof type === "string") return type;
    }
    return "";
  };
  if (Array.isArray(m["licenses"])) {
    return m["licenses"].map(one).filter(Boolean).join(" OR ");
  }
  return one(m["license"]);
}

const LICENSE_FILE = /^(licen[cs]e|copying|notice)([-._].*)?$/i;

/**
 * The license + notice files of a package folder, joined in name order ("" when there are none).
 * @param {string} dir
 * @returns {string}
 */
export function licenseText(dir) {
  const files = readdirSync(dir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && LICENSE_FILE.test(entry.name))
    .map((entry) => entry.name)
    .sort();
  return files
    .map((file) => readFileSync(join(dir, file), "utf8").replace(/\r\n/g, "\n").trim())
    .filter(Boolean)
    .join("\n\n");
}

/**
 * Collects the notices of the bundled npm packages from the lockfile and the installed node_modules.
 * @param {Lock} lock
 * @param {readonly string[]} roots
 * @param {string} modulesDir the app's node_modules folder
 * @returns {{ notices: Notice[], warnings: string[] }}
 */
export function npmNotices(lock, roots, modulesDir) {
  /** @type {Notice[]} */
  const notices = [];
  /** @type {string[]} */
  const warnings = [];
  for (const key of bundledKeys(lock, roots)) {
    const dir = join(modulesDir, key.slice("node_modules/".length));
    const manifestPath = join(dir, "package.json");
    if (!existsSync(manifestPath)) {
      // an optional dependency of another platform is in the lockfile but not installed here; anything else is a broken install
      if (lock.packages[key]?.optional) continue;
      throw new Error(`${key} is in package-lock.json but not installed (run npm ci)`);
    }
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    const name = typeof manifest.name === "string" ? manifest.name : nameOfKey(key);
    const version = typeof manifest.version === "string" ? manifest.version : "";
    const license = licenseOf(manifest);
    const text = licenseText(dir);
    if (license === "" && text === "") {
      throw new Error(`${name} ${version}: no license in package.json and no license file`);
    }
    if (text === "") warnings.push(`${name} ${version}: no license file (declares ${license})`);
    notices.push({ name, version, license, text });
  }
  return { notices, warnings };
}

/**
 * Notices -> one group per distinct license text (whitespace-insensitive), the packages using it listed. Packages
 * without a text form one group (text "").
 * @param {readonly Notice[]} notices
 * @returns {Group[]}
 */
export function groupNotices(notices) {
  /** @type {Map<string, { licenses: Set<string>, text: string, users: Set<string> }>} */
  const groups = new Map();
  for (const notice of notices) {
    const normalized = notice.text.replace(/\s+/g, " ").trim();
    const id =
      normalized === ""
        ? `none:${notice.license}`
        : createHash("sha256").update(normalized).digest("hex");
    let group = groups.get(id);
    if (!group) {
      group = { licenses: new Set(), text: notice.text, users: new Set() };
      groups.set(id, group);
    }
    if (notice.license) group.licenses.add(notice.license);
    group.users.add(`${notice.name} ${notice.version}`.trim());
  }
  return [...groups.values()]
    .map((group) => ({
      licenses: [...group.licenses].sort(),
      text: group.text,
      users: [...group.users].sort(),
    }))
    .sort(
      (a, b) =>
        a.licenses.join().localeCompare(b.licenses.join()) || b.users.length - a.users.length,
    );
}

/**
 * A code fence longer than any run of `~` inside the text, so license text can never close it.
 * @param {string} text
 * @returns {string}
 */
export function fenceFor(text) {
  const longest = Math.max(0, ...(text.match(/~+/g) ?? []).map((run) => run.length));
  return "~".repeat(Math.max(4, longest + 1));
}

/**
 * One group as Markdown (heading level 3).
 * @param {Group} group
 * @returns {string}
 */
export function renderGroup(group) {
  const title =
    group.licenses.length > 0 ? group.licenses.join(", ") : "License as the package ships it";
  const lines = [`### ${title}`, "", `Used by: ${group.users.join(", ")}`, ""];
  if (group.text === "") {
    lines.push(
      "These packages ship no license file; see their repositories for the license text.",
      "",
    );
  } else {
    const fence = fenceFor(group.text);
    lines.push(`${fence}text`, group.text, fence, "");
  }
  return lines.join("\n");
}

/**
 * The whole file.
 * @param {{ rust: string, npm: readonly Group[], libgit2: { version: string, text: string } }} parts
 * @returns {string}
 */
export function renderNotices(parts) {
  const fence = fenceFor(parts.libgit2.text);
  return [
    "# Third-party notices",
    "",
    "PerMark is free software under the MIT or the Apache-2.0 license, at your option (LICENSE-MIT and LICENSE-APACHE in its source repository, https://github.com/EdelmarSchneider/permark). It contains the third-party software below, each part under its own license. This file is made when PerMark is built, from the exact versions of that build.",
    "",
    "## Rust crates (compiled into the program)",
    "",
    "Crates under the Mozilla Public License 2.0 are used unmodified; their source code is on crates.io (https://crates.io/crates/<name>) and in each crate's repository.",
    "",
    parts.rust.trim(),
    "",
    "## npm packages (bundled in the interface, fonts included)",
    "",
    "Packages under the Eclipse Public License 2.0 or the Mozilla Public License 2.0 are used unmodified; their source code is on npmjs.com and in each package's repository.",
    "",
    parts.npm.map(renderGroup).join("\n").trim(),
    "",
    `## libgit2 ${parts.libgit2.version} (C library compiled into the program)`,
    "",
    "PerMark's git support uses libgit2 (https://libgit2.org, https://github.com/libgit2/libgit2), built into the program by the `libgit2-sys` crate, unmodified. libgit2 is free software under the GNU General Public License version 2 with a linking exception that allows linking it into other programs. Its license file follows, with the notices of the components bundled in it.",
    "",
    `${fence}text`,
    parts.libgit2.text.trim(),
    fence,
    "",
  ].join("\n");
}

// ---- The command ------------------------------------------------------------------------------------------------

/**
 * Runs a program and returns its output; a missing program or a failure says what to do.
 * @param {string} command
 * @param {readonly string[]} args
 * @param {string} cwd
 * @returns {string}
 */
function run(command, args, cwd) {
  try {
    return execFileSync(command, args, {
      cwd,
      encoding: "utf8",
      maxBuffer: 256 * 1024 * 1024,
      stdio: ["ignore", "pipe", "inherit"],
    });
  } catch (error) {
    const hint =
      command === "cargo" && args[0] === "about"
        ? " (is cargo-about installed? scripts/get-cargo-about.sh does it in CI)"
        : "";
    throw new Error(`${command} ${args.join(" ")} failed${hint}: ${String(error)}`);
  }
}

/** @param {string[]} argv */
function main(argv) {
  const here = dirname(fileURLToPath(import.meta.url));
  const app = resolve(here, "..");
  const tauri = join(app, "src-tauri");
  let out = join(tauri, "resources", "THIRD-PARTY-NOTICES.md");
  let modules = join(app, "node_modules");
  for (let i = 0; i < argv.length; i += 2) {
    const value = argv[i + 1];
    if (value === undefined) throw new Error(`${argv[i]} needs a value`);
    if (argv[i] === "--out") out = resolve(value);
    else if (argv[i] === "--modules") modules = resolve(value);
    else throw new Error(`unknown option ${argv[i]}`);
  }

  const manifest = JSON.parse(readFileSync(join(app, "package.json"), "utf8"));
  const lock = /** @type {Lock} */ (
    JSON.parse(readFileSync(join(app, "package-lock.json"), "utf8"))
  );
  const roots = [...Object.keys(manifest.dependencies ?? {}), ...BUNDLED_DEV];
  const { notices, warnings } = npmNotices(lock, roots, modules);
  for (const warning of warnings) console.warn(`warning: ${warning}`);

  // cargo-about refuses to write to a redirected stdout on Windows: it writes the file itself
  const scratch = mkdtempSync(join(tmpdir(), "permark-notices-"));
  const rustFile = join(scratch, "rust.md");
  run(
    "cargo",
    ["about", "generate", "--locked", "-c", "about.toml", "-o", rustFile, "about.hbs"],
    tauri,
  );
  const rust = readFileSync(rustFile, "utf8");
  rmSync(scratch, { recursive: true, force: true });
  if (!rust.includes("### ")) throw new Error("cargo about listed no license");

  const metadata = JSON.parse(
    run("cargo", ["metadata", "--format-version", "1", "--locked"], tauri),
  );
  const sys = metadata.packages.find(
    (/** @type {{ name: string }} */ p) => p.name === "libgit2-sys",
  );
  if (!sys) throw new Error("libgit2-sys is not in the dependency tree: update this script");
  const copying = join(dirname(sys.manifest_path), "libgit2", "COPYING");
  if (!existsSync(copying)) throw new Error(`${copying} not found`);
  const version = String(sys.version).split("+")[1] ?? String(sys.version);

  const file = renderNotices({
    rust,
    npm: groupNotices(notices),
    libgit2: { version, text: readFileSync(copying, "utf8").replace(/\r\n/g, "\n") },
  });
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, file);
  console.log(`${out}: ${(file.length / 1024).toFixed(0)} KiB, ${notices.length} npm packages`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
