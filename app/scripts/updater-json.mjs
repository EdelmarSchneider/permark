// Release `latest.json` (the manifest the in-app updater of every installed copy reads) from the assets of one GitHub
// release. Same shape, key order and values tauri-action (v0) wrote up to 0.6.1, minus the `.msi` keys (the `.msi` is
// no longer built); assembled once, after the three platform jobs uploaded their installers + `.sig` files in parallel
// (tauri-action's per-job merge races in parallel).
// Strict: every updater `.sig` maps to exactly one rule, every rule has exactly one `.sig` + its installer, and each
// installer verifies against its `.sig` with the updater key of tauri.conf.json -> else error (the release then stays a
// draft, nothing half-published). A signature made with another key would ship a `latest.json` every installed copy
// rejects.
//
//   node scripts/updater-json.mjs <release.json> <asset-dir> <owner/repo> <out-file>
//
// release.json = GitHub API release object; asset-dir = the release's `.sig` assets and their installers, downloaded
// under their asset names.
//
// The signing job of the release workflow also asks it which installers to sign (`bundles <dir>`) and checks what it
// signed before uploading (`verify <dir>`). Node built-ins only: that job runs no installed package.

import { createHash, createPublicKey, verify } from "node:crypto";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

/** @typedef {{ id: string, keyId: Buffer, key: import("node:crypto").KeyObject }} PublicKey */
/** @typedef {{ signature: string, url: string }} Platform */
/** @typedef {{ version: string, notes: string, pub_date: string, platforms: Record<string, Platform> }} UpdaterJson */
/** @typedef {{ name: string, pattern: RegExp, keys: readonly string[] }} Rule */

/**
 * Updater bundle (by its `.sig` asset name) -> `latest.json` platform keys, in tauri-action's order.
 * Windows: the NSIS `-setup.exe` holds every Windows key. Copies installed from an `.msi` look up `windows-x86_64-msi`,
 * then `windows-x86_64`, so the generic key MUST point at the `-setup.exe`: without it they find no update at all. The
 * updater picks the installer by the downloaded bytes, and the NSIS installer removes the `.msi` install first.
 * A stray `.msi.sig` then matches no rule and keeps the release a draft.
 * macOS: one universal bundle serves both architectures. Linux: AppImage = primary key.
 * @type {readonly Rule[]}
 */
export const RULES = [
  {
    name: "Windows NSIS",
    pattern: /_x64-setup\.exe\.sig$/,
    keys: ["windows-x86_64", "windows-x86_64-nsis"],
  },
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
 * Base64 text (as `tauri signer` writes it) -> its lines, without line ends.
 * @param {string} text
 * @returns {string[]}
 */
function lines(text) {
  return Buffer.from(text.trim(), "base64").toString("utf8").split(/\r?\n/);
}

/**
 * Minisign key id as minisign prints it: the 8 bytes little-endian, upper-case hex.
 * @param {Buffer} keyId
 * @returns {string}
 */
function keyIdText(keyId) {
  return Buffer.from(keyId).reverse().toString("hex").toUpperCase();
}

/**
 * `plugins.updater.pubkey` of tauri.conf.json (base64 of a minisign public key file) -> the Ed25519 key + its id.
 * @param {string} pubkey
 * @returns {PublicKey}
 */
export function publicKeyOf(pubkey) {
  const [comment, body] = lines(pubkey);
  const raw = Buffer.from(body ?? "", "base64");
  if (
    !comment?.startsWith("untrusted comment:") ||
    raw.length !== 42 ||
    raw.toString("latin1", 0, 2) !== "Ed"
  ) {
    throw new Error("the updater pubkey is not a minisign public key");
  }
  const keyId = raw.subarray(2, 10);
  const x = raw.subarray(10).toString("base64url");
  const key = createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x }, format: "jwk" });
  return { id: keyIdText(keyId), keyId, key };
}

/**
 * Checks a `.sig` asset (base64 of a minisign signature file, as `tauri signer sign` writes it) against the file it
 * signs, the way the in-app updater does: Ed25519 over the file's BLAKE2b-512 hash, and over the signature + trusted
 * comment. Throws when anything does not match.
 * @param {string} signature the `.sig` text
 * @param {Uint8Array} data the signed file
 * @param {string} pubkey `plugins.updater.pubkey` of tauri.conf.json
 */
export function verifySignature(signature, data, pubkey) {
  const key = publicKeyOf(pubkey);
  const [comment, body, trusted, global] = lines(signature);
  const raw = Buffer.from(body ?? "", "base64");
  const globalSig = Buffer.from(global ?? "", "base64");
  if (
    !comment?.startsWith("untrusted comment:") ||
    !trusted?.startsWith("trusted comment: ") ||
    raw.length !== 74 ||
    globalSig.length !== 64
  ) {
    throw new Error("not a minisign signature");
  }
  // "ED" = signed over the BLAKE2b-512 hash of the file: what `tauri signer` writes and the updater checks
  const algorithm = raw.toString("latin1", 0, 2);
  if (algorithm !== "ED")
    throw new Error(`unexpected signature algorithm ${JSON.stringify(algorithm)}`);
  const keyId = raw.subarray(2, 10);
  if (!keyId.equals(key.keyId)) {
    throw new Error(`signed with key ${keyIdText(keyId)}, but the app trusts key ${key.id}`);
  }
  const sig = raw.subarray(10);
  const hash = createHash("blake2b512").update(data).digest();
  if (!verify(null, hash, key.key, sig)) throw new Error("the signature does not match the file");
  const signedComment = Buffer.concat([
    sig,
    Buffer.from(trusted.slice("trusted comment: ".length)),
  ]);
  if (!verify(null, signedComment, key.key, globalSig)) {
    throw new Error("the trusted comment does not match its signature");
  }
}

/**
 * Asset names + signature texts -> `latest.json` object. `verify` checks one installer against its signature text and
 * throws when they do not match.
 * @param {{ version: string, notes: string, tag: string, repo: string, pubDate: string, assets: readonly string[],
 *   signature: (assetName: string) => string, verify: (bundle: string, signature: string) => void }} input
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
    try {
      input.verify(bundle, signature);
    } catch (error) {
      throw new Error(
        `${rule.name}: ${sig}: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
    for (const key of rule.keys)
      platforms[key] = { signature, url: base + encodeURIComponent(bundle) };
  }
  return { version: input.version, notes: input.notes, pub_date: input.pubDate, platforms };
}

/**
 * File names -> the updater bundles among them (the installers a rule signs: not the `.dmg`), in name order. The release
 * workflow signs exactly these, so what it signs and what `latest.json` needs come from the same rules.
 * @param {readonly string[]} names
 * @returns {string[]}
 */
export function updaterBundles(names) {
  return names.filter((name) => RULES.some((rule) => rule.pattern.test(`${name}.sig`))).sort();
}

/**
 * The signing job's check before anything is uploaded: every updater bundle among `names` has its `.sig` beside it and
 * `verify` accepts it; a `.sig` of anything else is refused too (`latest.json` would refuse it later and keep the release
 * a draft). Returns the bundles checked; throws on the first problem, naming the file.
 * @param {{ names: readonly string[], signature: (sigName: string) => string,
 *   verify: (bundle: string, signature: string) => void }} input
 * @returns {string[]}
 */
export function checkSigned(input) {
  const names = new Set(input.names);
  const bundles = updaterBundles(input.names);
  if (bundles.length === 0) throw new Error("no updater bundle to check");
  for (const name of input.names) {
    if (name.endsWith(".sig") && !bundles.includes(name.slice(0, -".sig".length))) {
      throw new Error(`${name} signs no updater bundle`);
    }
  }
  for (const bundle of bundles) {
    const sig = `${bundle}.sig`;
    if (!names.has(sig)) throw new Error(`${bundle} has no signature ${sig}`);
    const signature = input.signature(sig);
    if (signature.trim() === "") throw new Error(`${sig} is empty`);
    try {
      input.verify(bundle, signature);
    } catch (error) {
      throw new Error(`${sig}: ${error instanceof Error ? error.message : String(error)}`);
    }
  }
  return bundles;
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

/** @returns {{ version: string, pubkey: string }} the app's version and updater key, from tauri.conf.json */
function appConfig() {
  const conf = JSON.parse(
    readFileSync(fileURLToPath(new URL("../src-tauri/tauri.conf.json", import.meta.url)), "utf8"),
  );
  const pubkey = conf.plugins?.updater?.pubkey;
  if (typeof pubkey !== "string") throw new Error("tauri.conf.json has no plugins.updater.pubkey");
  return { version: String(conf.version), pubkey };
}

const USAGE = `usage:
  node scripts/updater-json.mjs <release.json> <asset-dir> <owner/repo> <out-file>
  node scripts/updater-json.mjs bundles <dir>   (lists the updater bundles in <dir>)
  node scripts/updater-json.mjs verify <dir>    (checks their .sig files with the app's updater key)`;

function main() {
  const [command, dir] = process.argv.slice(2);
  if (command === "bundles" || command === "verify") {
    if (!dir) throw new Error(USAGE);
    const names = readdirSync(dir);
    if (command === "bundles") {
      for (const name of updaterBundles(names)) console.log(name);
      return;
    }
    const { pubkey } = appConfig();
    const checked = checkSigned({
      names,
      signature: (name) => readFileSync(join(dir, name), "utf8"),
      verify: (bundle, signature) =>
        verifySignature(signature, readFileSync(join(dir, bundle)), pubkey),
    });
    const key = publicKeyOf(pubkey).id;
    for (const bundle of checked) console.log(`${bundle}: signature verified with key ${key}`);
    return;
  }
  const [releasePath, assetDir, repo, outPath] = process.argv.slice(2);
  if (!releasePath || !assetDir || !repo || !outPath) throw new Error(USAGE);
  const { version, pubkey } = appConfig();
  const release = parseRelease(readFileSync(releasePath, "utf8"));
  const json = updaterJson({
    version,
    notes: release.notes,
    tag: release.tag,
    repo,
    pubDate: new Date().toISOString(),
    assets: release.assets,
    signature: (name) => readFileSync(join(assetDir, name), "utf8"),
    verify: (bundle, signature) =>
      verifySignature(signature, readFileSync(join(assetDir, bundle)), pubkey),
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
