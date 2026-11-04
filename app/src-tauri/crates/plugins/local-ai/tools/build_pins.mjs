// Prints the pins of model files for `src/catalog.json`: size + LFS sha256 of each file at one commit of a
// Hugging Face repo. Dev tool only (the app never asks the HF API; pins ride releases).
//
//     node tools/build_pins.mjs <publisher/repo> <commit|main> <path> [<path>...]
//
// `main` -> resolves the repo's current commit sha first (pin that sha, never a branch). Output (stdout, JSON):
// { repo, commit, files: [{ path, bytes, sha256 }] }. Fails (exit 1) when a path is missing or not stored in LFS.
// Node 18+ (global fetch), no dependencies.

const API = "https://huggingface.co/api/models";

async function json(url) {
  const res = await fetch(url, { headers: { accept: "application/json" } });
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return res.json();
}

async function commitOf(repo, ref) {
  if (/^[0-9a-f]{40}$/.test(ref)) return ref;
  const info = await json(`${API}/${repo}/revision/${encodeURIComponent(ref)}`);
  if (!/^[0-9a-f]{40}$/.test(info.sha ?? "")) throw new Error(`${repo}@${ref}: no commit sha`);
  return info.sha;
}

async function main() {
  const [repo, ref, ...paths] = process.argv.slice(2);
  if (!repo || !ref || paths.length === 0) {
    console.error("usage: node tools/build_pins.mjs <publisher/repo> <commit|main> <path> [<path>...]");
    process.exit(2);
  }
  const commit = await commitOf(repo, ref);
  const tree = await json(`${API}/${repo}/tree/${commit}?recursive=1`);
  const byPath = new Map(tree.map((entry) => [entry.path, entry]));
  const files = paths.map((path) => {
    const entry = byPath.get(path);
    if (!entry || entry.type !== "file") throw new Error(`${repo}@${commit}: no file ${path}`);
    const sha256 = entry.lfs?.oid;
    if (!/^[0-9a-f]{64}$/.test(sha256 ?? "")) throw new Error(`${path}: not in LFS (no sha256)`);
    return { path, bytes: entry.lfs.size ?? entry.size, sha256 };
  });
  console.log(JSON.stringify({ repo, commit, files }, null, 2));
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
});
