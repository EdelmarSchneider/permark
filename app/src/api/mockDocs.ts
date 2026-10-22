// Documents for the mock backend (dev:mock, UI tests). Markdown views come from the REAL renderer: `cargo test` renders
// the fixture documents (`fixtures/*.md`) with every plugin on and writes `generated/mock-documents.json`; this file
// only serves them (blocks the caller already holds leave out their HTML) and makes up the simple ones (text, picture).

import mockDocuments from "./generated/mock-documents.json";
import type { Block, DocumentView, LinkTarget, OutlineItem } from "./types";

function isView(value: unknown): value is DocumentView {
  if (typeof value !== "object" || value === null) return false;
  return "path" in value && "body" in value && "format" in value;
}

const FIXTURES = new Map<string, DocumentView>(
  Object.entries(mockDocuments).flatMap(([path, view]): [string, DocumentView][] =>
    isView(view) ? [[path, view]] : [],
  ),
);

export const MOCK_DOCUMENT_PATHS: readonly string[] = [...FIXTURES.keys()];

const TEXTS: Record<string, { text: string; language: string; format: string }> = {
  "package.json": {
    text: '{\n  "name": "permark-docs",\n  "version": "1.0.0",\n  "scripts": { "build": "docs build" },\n  "keywords": ["docs", "markdown"],\n  "private": true\n}\n',
    language: "json",
    format: "json",
  },
  "notes/todo.txt": {
    text: "todo\n- read the guide\n- write the next section\n- check the build log:\n  2026-10-22T09:14:03.512Z INFO [runner] step 4/7 finished: built 1284 modules in 38.2 s, cache hits 1120 (87.2%), warnings 3 (unused import in src/render/page.ts:41, deprecated option 'legacyAnchors' in docs.config.json, slow plugin 'mermaid' took 6.1 s), output written to dist/site with 412 pages, 96 images and 18 downloads; next step: link check over 3 018 links (external links skipped: offline mode)\n",
    language: "txt",
    format: "text",
  },
  "src/main.rs": {
    text: 'fn main() {\n    println!("hello");\n}\n',
    language: "rs",
    format: "text",
  },
  ".github/ci.yml": { text: "name: ci\non: push\n", language: "yml", format: "text" },
};

/** Text of a mock text file (not Markdown), or `null`. */
export function mockTextOf(path: string): string | null {
  return TEXTS[path]?.text ?? null;
}

function nameOf(path: string): string {
  return path.split("/").at(-1) ?? path;
}

function base(path: string): Omit<DocumentView, "body" | "format" | "editable"> {
  return {
    path,
    name: nameOf(path),
    role: null,
    hash: "0000000000000000",
    size: 1024,
    mtimeMs: Date.now() - 180_000,
    notes: [],
  };
}

/** The view of a mock document, or `null` when the mock has no such file. `known`: block ids the caller holds. */
export function mockView(
  path: string,
  known: readonly string[],
  exists: boolean,
): DocumentView | null {
  const fixture = FIXTURES.get(path);
  if (fixture) {
    const held = new Set(known);
    const body =
      fixture.body.kind === "markdown"
        ? {
            ...fixture.body,
            blocks: fixture.body.blocks.map((b) => (held.has(b.id) ? { ...b, html: null } : b)),
          }
        : fixture.body;
    return { ...fixture, body, mtimeMs: Date.now() - 180_000 };
  }
  const text = TEXTS[path];
  if (text) {
    return {
      ...base(path),
      format: text.format,
      body: { kind: "text", text: text.text, language: text.language },
      editable: { editable: "surgical" },
    };
  }
  if (/\.(png|jpe?g|gif|webp|svg)$/i.test(path)) {
    return {
      ...base(path),
      hash: null,
      format: "media",
      body: { kind: "media", media: "image", mime: "image/png" },
      editable: { editable: "readOnly", reason: "media" },
    };
  }
  if (!exists) return null;
  return {
    ...base(path),
    hash: null,
    format: "text",
    body: { kind: "notice", notice: { notice: "binary" } },
    editable: { editable: "readOnly", reason: "binary" },
  };
}

/** A picture the browser can show with no server: a labelled placeholder. */
export function mockAssetUrl(path: string): string {
  const label = nameOf(path).replace(/[<>&"]/g, "");
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="320" height="120" viewBox="0 0 320 120"><rect width="320" height="120" rx="10" fill="#7b93ff" opacity="0.25"/><text x="160" y="66" font-family="sans-serif" font-size="16" text-anchor="middle" fill="#4f6bed">${label}</text></svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** Where a link leads, by the mock project's files (a simplified `links::resolve`). */
export function mockLinkTarget(
  doc: string,
  href: string,
  exists: (path: string) => boolean,
): LinkTarget {
  const value = href.trim();
  if (value.startsWith("#")) return { type: "anchor", id: decodeURIComponent(value.slice(1)) };
  if (/^[a-z][a-z0-9+.-]*:/i.test(value)) {
    return /^(javascript|data|vbscript):/i.test(value)
      ? { type: "missing", path: value }
      : { type: "web", url: value };
  }
  const [target, anchor] = value.split("#");
  const folder = doc.split("/").slice(0, -1);
  const parts = target?.startsWith("/") ? [] : folder;
  for (const part of decodeURIComponent(target ?? "").split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      // above the folder of the workspace: never served
      if (parts.length === 0) return { type: "outside" };
      parts.pop();
    } else parts.push(part);
  }
  const joined = parts.join("/");
  for (const candidate of [joined, `${joined}.md`]) {
    if (exists(candidate)) {
      return /\.(md|markdown)$/i.test(candidate)
        ? { type: "doc", path: candidate, anchor: anchor ?? null }
        : { type: "asset", path: candidate };
    }
  }
  return { type: "missing", path: joined };
}

const ESCAPES: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;" };

/** `inline`: inline code and links of the text become `code` | `a` (a Markdown file a test added). */
function inlineOf(safe: string): string {
  return safe
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_, label: string, href: string) => {
      const quoted = href.replace(/"/g, "&quot;");
      return `<a href="${quoted}">${label}</a>`;
    });
}

/** A simple view of a note's text for the mock: a heading or a paragraph per run of lines (no renderer here). */
export function mockNoteView(
  path: string,
  text: string,
  options: { inline?: boolean } = {},
): DocumentView {
  const lines = text.split("\n");
  const blocks: Block[] = [];
  const outline: OutlineItem[] = [];
  let start = -1;
  const close = (end: number): void => {
    const run = lines.slice(start, end).join("\n");
    const id = `n${blocks.length}-${run.length}`;
    const heading = /^(#{1,6})\s+(.*)$/.exec(run);
    const escaped = (heading?.[2] ?? run)
      .replace(/[&<>]/g, (c) => ESCAPES[c] ?? c)
      .replace(/\n/g, "<br>");
    const safe = options.inline === true ? inlineOf(escaped) : escaped;
    const tag = heading ? `h${heading[1]?.length ?? 1}` : "p";
    if (heading) {
      outline.push({
        id: `h-${blocks.length}`,
        level: heading[1]?.length ?? 1,
        label: heading[2] ?? "",
        line: start + 1,
        block: id,
      });
    }
    blocks.push({
      id,
      kind: tag,
      lineStart: start + 1,
      lineEnd: end,
      html: `<${tag} data-b="${id}">${safe}</${tag}>`,
    });
    start = -1;
  };
  lines.forEach((line, i) => {
    const filled = line.trim() !== "";
    if (filled && start < 0) start = i;
    if (!filled && start >= 0) close(i);
  });
  if (start >= 0) close(lines.length);
  return {
    ...base(path),
    format: "markdown",
    body: { kind: "markdown", blocks, outline, frontMatter: null },
    editable: { editable: "surgical" },
    size: text.length,
  };
}
