// Release `latest.json` (the manifest the in-app updater of every installed copy reads) from the assets of one GitHub
// release. Same shape, key order and values tauri-action (v0) wrote up to 0.6.1; assembled once, after the three
// platform jobs uploaded their installers + `.sig` files in parallel (tauri-action's per-job merge races in parallel).
// Strict: every updater `.sig` maps to exactly one rule, every rule has exactly one `.sig` + its installer -> else error
// (the release then stays a draft, nothing half-published).
//
//   node scripts/updater-json.mjs <release.json> <sig-dir> <owner/repo> <out-file>
//
// release.json = GitHub API release object; sig-dir = the release's `.sig` assets, downloaded under their asset names.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

/** @typedef {{ signature: string, url: string }} Platform */
/** @typedef {{ version: string, notes: string, pub_date: string, platforms: Record<string, Platform> }} UpdaterJson */
/** @typedef {{ name: string, pattern: RegExp, keys: readonly string[] }} Rule */

/**
 * Updater bundle (by its `.sig` asset name) -> `latest.json` platform keys, in tauri-action's order.
 * Windows: `.msi` = primary key (tauri-action default, `updaterJsonPreferNsis` off). macOS: one universal bundle serves
 * both architectures. Linux: AppImage = primary key.
 * @type {readonly Rule[]}
 */
export const RULES = [
  {
    name: "Windows MSI",
    pattern: /_x64_[A-Za-z-]+\.msi\.sig$/,
    keys: ["windows-x86_64", "windows-x86_64-msi"],
  },
  { name: "Windows NSIS", pattern: /_x64-setup\.exe\.sig$/, keys: ["windows-x86_64-nsis"] },
  {
    name: "macOS universal app",
    pattern: /_universal\.app\.tar\.gz\.sig$/,
    keys: ["darwin-aarch64", "darwin-x86_64", "darwin-aarch64-app", "darwin-x86_64-app"],
  },
  {
    name: "Linux AppImage",
    pattern: /_amd64\.AppImage\.sig$/,
    keys: ["linux-x86_64", "linux-x86_64-appimage"],
  },
  { name: "Linux deb", pattern: /_amd64\.deb\.sig$/, keys: ["linux-x86_64-deb"] },
  { name: "Linux rpm", pattern: /\.x86_64\.rpm\.sig$/, keys: ["linux-x86_64-rpm"] },
];

/**
 * Asset names + signature texts -> `latest.json` object.
 * @param {{ version: string, notes: string, tag: string, repo: string, pubDate: string, assets: readonly string[],
 *   signature: (assetName: string) => string }} input
 * @returns {UpdaterJson}
 */
export function updaterJson(input) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(input.repo))
    throw new Error(`repo ${JSON.stringify(input.repo)} is not owner/name`);
  if (input.tag !== `v${input.version}`) {
    throw new Error(`release tag ${input.tag} is not the app's version v${input.version}`);
  }
  const names = new Set(input.assets);
  const sigs = input.assets.filter((name) => name.endsWith(".sig"));
  for (const sig of sigs) {
    const matches = RULES.filter((rule) => rule.pattern.test(sig));
    if (matches.length !== 1)
      throw new Error(`signature ${sig} matches ${matches.length} updater rules (want 1)`);
  }
  /** @type {Record<string, Platform>} */
  const platforms = {};
  const base = `https://github.com/${input.repo}/releases/download/${encodeURIComponent(input.tag)}/`;
  for (const rule of RULES) {
    const found = sigs.filter((sig) => rule.pattern.test(sig));
    const sig = found[0];
    if (found.length !== 1 || sig === undefined) {
      throw new Error(`${rule.name}: ${found.length} signature assets (want 1)`);
    }
    const bundle = sig.slice(0, -".sig".length);
    if (!names.has(bundle)) throw new Error(`${rule.name}: ${sig} has no asset ${bundle}`);
    const signature = input.signature(sig);
    if (signature.trim() === "") throw new Error(`${rule.name}: ${sig} is empty`);
    for (const key of rule.keys)
      platforms[key] = { signature, url: base + encodeURIComponent(bundle) };
  }
  return { version: input.version, notes: input.notes, pub_date: input.pubDate, platforms };
}

/**
 * GitHub API release object -> its tag, body and asset names.
 * @param {string} text
 * @returns {{ tag: string, notes: string, assets: string[] }}
 */
export function parseRelease(text) {
  /** @type {unknown} */
  const parsed = JSON.parse(text);
  if (typeof parsed !== "object" || parsed === null) throw new Error("release.json: not an object");
  const { tag_name: tag, body, assets } = /** @type {Record<string, unknown>} */ (parsed);
  if (typeof tag !== "string") throw new Error("release.json: no tag_name");
  if (!Array.isArray(assets)) throw new Error("release.json: no assets");
  const names = assets.map((asset) => {
    const name =
      typeof asset === "object" && asset !== null
        ? /** @type {{ name?: unknown }} */ (asset).name
        : null;
    if (typeof name !== "string" || name === "" || /[\\/]/.test(name)) {
      throw new Error(`release.json: bad asset name ${JSON.stringify(name)}`);
    }
    return name;
  });
  return { tag, notes: typeof body === "string" ? body : "", assets: names };
}

function main() {
  const [releasePath, sigDir, repo, outPath] = process.argv.slice(2);
  if (!releasePath || !sigDir || !repo || !outPath) {
    throw new Error(
      "usage: node scripts/updater-json.mjs <release.json> <sig-dir> <owner/repo> <out-file>",
    );
  }
  const conf = JSON.parse(
    readFileSync(fileURLToPath(new URL("../src-tauri/tauri.conf.json", import.meta.url)), "utf8"),
  );
  const release = parseRelease(readFileSync(releasePath, "utf8"));
  const json = updaterJson({
    version: String(conf.version),
    notes: release.notes,
    tag: release.tag,
    repo,
    pubDate: new Date().toISOString(),
    assets: release.assets,
    signature: (name) => readFileSync(join(sigDir, name), "utf8"),
  });
  writeFileSync(outPath, JSON.stringify(json, null, 2));
  console.log(`${outPath}: ${Object.keys(json.platforms).join(", ")}`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
