// The editing half of the browser mock: the same commands as the Rust backend (`document_source`, `parse_blocks`,
// `save_document`, `create_document`, `write_asset`), over the mock documents. Errors carry the same `kind`s.

import { emitMock } from "../api";
import { basename, dirname } from "../paths";
import { hashString } from "../text";
import type { AssetWritten, CreatedDoc, ParsedBlocks, SaveOutcome, SourceDoc } from "../types";
import {
  allDocPaths,
  docExists,
  mockDocKind,
  mockEncodingNote,
  mockFiles,
  mockSnapshot,
  mockState,
  sourceMtime,
  sourceText,
  writeSource,
} from "./fixtures";
import { mockParseBlocks } from "./mockBlocks";
import { markdownToView } from "./mockMarkdown";

const wait = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/** What the backend rejects with: the serialized `AppError`, `{ kind, message }`. */
export class MockBackendError extends Error {
  constructor(
    readonly kind: string,
    message: string,
  ) {
    super(message);
  }
}

/** The Markdown source of every mock document (the corpus of the round-trip test). */
export function mockSources(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const path of allDocPaths()) {
    if (mockDocKind(path) !== "markdown") continue;
    const text = sourceText(path);
    if (text !== null) out[path] = text;
  }
  return out;
}

/** The hash the mock uses for a file's content. */
export const mockHash = (text: string): string => hashString(text);

export async function documentSource(path: string): Promise<SourceDoc> {
  await wait(40);
  const kind = mockDocKind(path);
  if (kind === null) throw new MockBackendError("notFound", `not found: ${path}`);
  if (kind === "binary") throw new MockBackendError("binary", `${path} is a binary file`);
  if (kind === "tooLarge") throw new MockBackendError("tooLarge", `${path} is too large to edit`);
  if (kind !== "markdown")
    throw new MockBackendError("notMarkdown", `${path} is not a Markdown file`);
  if (mockEncodingNote(path)) throw new MockBackendError("encoding", `${path} is not UTF-8 text`);
  const text = sourceText(path) ?? "";
  return {
    path,
    text,
    hash: mockHash(text),
    modifiedMs: sourceMtime(path),
    ...mockParseBlocks(text),
  };
}

export async function parseBlocks(text: string): Promise<ParsedBlocks> {
  await wait(10);
  return mockParseBlocks(text);
}

/** The reading view's HTML for pieces of Markdown (what Visual mode draws its locked blocks with). */
export async function renderFragments(texts: string[]): Promise<string[]> {
  await wait(10);
  return texts.map((text) => markdownToView(text).html);
}

export async function saveDocument(
  path: string,
  text: string,
  expectedHash: string,
): Promise<SaveOutcome> {
  await wait(120);
  if (mockFiles.failNextSave) {
    mockFiles.failNextSave = false;
    throw new MockBackendError("denied", `cannot write ${path}: Access is denied. (os error 5)`);
  }
  if (mockDocKind(path) === null) return { result: "conflict", diskHash: null };
  const current = sourceText(path) ?? "";
  if (mockHash(current) !== expectedHash) {
    return { result: "conflict", diskHash: mockHash(current) };
  }
  const modifiedMs = writeSource(path, text);
  emitMock("git-status-changed", mockSnapshot());
  return { result: "saved", hash: mockHash(text), modifiedMs };
}

const FORBIDDEN_NAME = /[\\/:*?"<>|\u0000-\u001f]/;

export async function createDocument(path: string, text: string): Promise<CreatedDoc> {
  await wait(60);
  const name = basename(path);
  if (!/\.(md|markdown|mdown|mkd|mdx)$/i.test(name) || name.startsWith(".")) {
    throw new MockBackendError("notMarkdown", `${path} is not a Markdown file`);
  }
  if (FORBIDDEN_NAME.test(name) || /[. ]$/.test(name)) {
    throw new MockBackendError("invalidName", "Names can't contain \\ / : * ? \" < > |");
  }
  const exists = [...allDocPaths(), ...mockFiles.created].some(
    (p) => p.toLowerCase() === path.toLowerCase(),
  );
  if (exists && !mockState.deleted.has(path)) {
    throw new MockBackendError("exists", `a file named ${name} already exists`);
  }
  mockFiles.created.add(path);
  mockState.deleted.delete(path);
  const modifiedMs = writeSource(path, text);
  emitMock("tree-changed", null);
  emitMock("git-status-changed", mockSnapshot());
  return { path, hash: mockHash(text), modifiedMs };
}

const IMAGE = /\.(png|jpe?g|gif|webp|svg)$/i;
/** Names Windows opens as devices; the backend adds "-image" to an image stem like these. */
const DEVICE_NAME = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/;

export async function writeAsset(
  docPath: string,
  nameHint: string,
  data: string,
): Promise<AssetWritten> {
  await wait(60);
  const name = basename(nameHint.replace(/\\/g, "/"));
  if (!IMAGE.test(name)) {
    throw new MockBackendError(
      "invalidName",
      "Only images can be added here (PNG, JPG, GIF, WebP, SVG)",
    );
  }
  const dot = name.lastIndexOf(".");
  const slug =
    name
      .slice(0, dot)
      .toLowerCase()
      .replace(/[^\p{L}\p{N}]+/gu, "-")
      .replace(/^-+|-+$/g, "") || "image";
  const stem = DEVICE_NAME.test(slug) ? `${slug}-image` : slug;
  const ext = name.slice(dot + 1).toLowerCase();
  const folder = dirname(docPath);
  const prefix = folder === "" ? "assets" : `${folder}/assets`;
  let n = 1;
  let file = `${stem}.${ext}`;
  while (mockFiles.assets.has(`${prefix}/${file}`)) file = `${stem}-${++n}.${ext}`;
  mockFiles.assets.add(`${prefix}/${file}`);
  return {
    path: `${prefix}/${file}`,
    rel: `assets/${file}`,
    bytes: Math.floor((data.length * 3) / 4),
  };
}
