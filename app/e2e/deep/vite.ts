// The deep suite's server (`vite --mode deep`): the mock backend with the rendered corpus in place of the generated mock
// documents, the corpus's files (pictures, sound) served as they are, and the app's Content Security Policy on every
// response, as the app's web view enforces it. Serve only: a build never holds any of it.

import { createReadStream, existsSync, readFileSync, realpathSync, statSync } from "node:fs";
import path from "node:path";
import { searchForWorkspaceRoot, type Plugin } from "vite";
import { APP_DIR, corpusOut, DEEP_MODE, deepHome, FILES_ROUTE } from "./paths.ts";

/** Generated mock fixture -> the rendered corpus file that stands in for it. */
const STAND_INS: Record<string, string> = {
  "mock-documents.json": "documents.json",
  "mock-edit-sources.json": "edit-sources.json",
  "mock-config-sheets.json": "config-sheets.json",
};

const MOCK_FIXTURE = /(?:^|[\\/])generated[\\/](mock-[a-z-]+\.json)$/;

const TYPES: Record<string, string> = {
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".webp": "image/webp",
  ".avif": "image/avif",
  ".wav": "audio/wav",
  ".mp3": "audio/mpeg",
  ".ogg": "audio/ogg",
  ".m4a": "audio/mp4",
  ".flac": "audio/flac",
  ".mp4": "video/mp4",
  ".webm": "video/webm",
  ".mov": "video/quicktime",
};

function field(value: unknown, key: string): unknown {
  return typeof value === "object" && value !== null ? Reflect.get(value, key) : undefined;
}

/** The app's CSP (`tauri.conf.json`), as its web view enforces it. */
export function appCsp(): string {
  const conf: unknown = JSON.parse(
    readFileSync(path.join(APP_DIR, "src-tauri", "tauri.conf.json"), "utf8"),
  );
  const csp = field(field(field(conf, "app"), "security"), "csp");
  if (typeof csp !== "string" || csp === "") throw new Error("no CSP in tauri.conf.json");
  return csp;
}

/** The file of the corpus project at `url` (under FILES_ROUTE), or `null` when it names nothing inside it. */
export function corpusFile(project: string, url: string): string | null {
  const rel = url.slice(FILES_ROUTE.length).split(/[?#]/)[0] ?? "";
  let decoded: string;
  try {
    decoded = decodeURIComponent(rel);
  } catch {
    return null;
  }
  const file = path.resolve(project, decoded);
  const inside = path.relative(project, file);
  if (inside === "" || inside.startsWith("..") || path.isAbsolute(inside)) return null;
  return existsSync(file) && statSync(file).isFile() ? file : null;
}

export function deepCorpus(): Plugin {
  const out = corpusOut();
  const project = path.join(out, "project");
  return {
    name: "permark-deep-corpus",
    // before Vite's own resolver, which would find the generated fixtures first
    enforce: "pre",
    apply: (_config, env) => env.mode === DEEP_MODE && env.command === "serve",
    config() {
      if (!existsSync(path.join(out, "documents.json"))) {
        throw new Error(`no rendered corpus in ${out}: run \`npm run deep\` (it renders it first)`);
      }
      const cacheDir = path.join(deepHome(), "vite-cache");
      // the rendered corpus and the server's own dependency cache live outside the app; node_modules may be a link
      return {
        cacheDir,
        server: {
          fs: {
            allow: [
              searchForWorkspaceRoot(APP_DIR),
              realpathSync(path.join(APP_DIR, "node_modules")),
              out,
              cacheDir,
            ],
          },
        },
      };
    },
    resolveId(id) {
      const name = MOCK_FIXTURE.exec(id)?.[1];
      const standIn = name !== undefined ? STAND_INS[name] : undefined;
      return standIn !== undefined ? path.join(out, standIn) : null;
    },
    configureServer(server) {
      const csp = appCsp();
      server.middlewares.use((req, res, next) => {
        res.setHeader("Content-Security-Policy", csp);
        const url = req.url ?? "";
        if (!url.startsWith(FILES_ROUTE)) return next();
        const file = corpusFile(project, url);
        if (file === null) {
          res.statusCode = 404;
          res.end();
          return;
        }
        res.setHeader(
          "Content-Type",
          TYPES[path.extname(file).toLowerCase()] ?? "application/octet-stream",
        );
        res.setHeader("Content-Length", String(statSync(file).size));
        createReadStream(file).pipe(res);
      });
    },
  };
}
