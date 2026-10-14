// The documents of the round-trip corpus and what is known about them, shared by the corpus tests (`corpus.test.ts` for the block
// model, `visual/corpus.test.ts` for Visual mode). Nothing here is a test: it reads the frozen snapshot of the project's own
// documents (`corpus/docs/`), the mock fixtures and the hostile set, and the ranges the real parser found (`corpus/ranges.json`).
// See the header of `corpus.test.ts` for how the fixtures are regenerated.

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { mockParseBlocks } from "../mock/mockBlocks";
import { mockSources } from "../mock/mockSource";
import type { BlockKind, ParsedBlocks } from "../types";
import { BOM, type BlockMap } from "./blockModel";
import { HOSTILE } from "./hostileFixtures";

export type Doc = { name: string; text: string };

export const UPDATE = process.env.UPDATE_CORPUS === "1";
export const INLINE = "src/lib/editor/corpus/inline.json";
export const RANGES = "src/lib/editor/corpus/ranges.json";
const FROZEN = "src/lib/editor/corpus/docs";
export const REGENERATE =
  "regenerate with: UPDATE_CORPUS=1 npx vitest run src/lib/editor/corpus.test.ts && " +
  "UPDATE_CORPUS=1 cargo test --manifest-path src-tauri/Cargo.toml corpus";
/** Built from repeated text at run time; they are about size, which the Rust tests cover, and would only bloat the fixtures. */
export const GENERATED = new Set(["hostile: very long lines", "hostile: many tiny blocks"]);

/** The Markdown files of the snapshot, named by their path inside it. */
function snapshotFiles(dir = ""): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(join(FROZEN, dir), { withFileTypes: true })) {
    const name = dir === "" ? entry.name : `${dir}/${entry.name}`;
    if (entry.isDirectory()) out.push(...snapshotFiles(name));
    else if (entry.name.endsWith(".md")) out.push(name);
  }
  return out;
}

export const snapshot: Doc[] = snapshotFiles()
  .sort()
  .map((name) => ({ name, text: readFileSync(join(FROZEN, name), "utf8") }));
export const inline: Doc[] = [
  ...Object.entries(mockSources()).map(([name, text]) => ({ name: `mock ${name}`, text })),
  ...Object.entries(HOSTILE).map(([name, text]) => ({ name: `hostile: ${name}`, text })),
];
export const docs: Doc[] = [...snapshot, ...inline];
// the same documents as a Windows editor would have saved them
export const windows: Doc[] = docs.map(({ name, text }) => ({
  name: `${name} (CRLF, BOM)`,
  text: BOM + text.replace(/\r\n|\r|\n/g, "\r\n"),
}));
export const everything = [...docs, ...windows];

// ---- the real ranges ---------------------------------------------------------------------------------------

type RangeEntry = {
  name: string;
  bytes: number;
  fnv: number;
  eol: ParsedBlocks["eol"];
  blocks: [number, number, BlockKind][];
};

const encoder = new TextEncoder();

/** FNV-1a over the UTF-8 bytes; the Rust side computes the same number. */
export function fingerprint(text: string): number {
  let hash = 0x811c9dc5;
  for (const byte of encoder.encode(text)) hash = Math.imul(hash ^ byte, 0x01000193);
  return hash >>> 0;
}

function readRanges(): Map<string, RangeEntry> {
  if (!existsSync(RANGES)) return new Map();
  const entries = JSON.parse(readFileSync(RANGES, "utf8")) as RangeEntry[];
  return new Map(entries.map((entry) => [entry.name, entry]));
}

const ranges = readRanges();

/** Whether the fixture has this document as it is now. */
export function isCurrent(doc: Doc): boolean {
  const entry = ranges.get(doc.name);
  return (
    entry !== undefined &&
    entry.bytes === encoder.encode(doc.text).length &&
    entry.fnv === fingerprint(doc.text)
  );
}

export function realRanges(doc: Doc): ParsedBlocks {
  const entry = ranges.get(doc.name) as RangeEntry;
  return {
    eol: entry.eol,
    blocks: entry.blocks.map(([startByte, endByte, kind]) => ({ startByte, endByte, kind })),
  };
}

// ---- helpers of the checks ---------------------------------------------------------------------------------

export const withoutBom = (text: string): string =>
  text.startsWith(BOM) ? text.slice(BOM.length) : text;

/** The lines of a text that are not blank, without their terminators. */
export function contentLines(text: string): string[] {
  return text.split(/\r\n|\r|\n/).filter((line) => line.trim() !== "");
}

/** The non-blank lines that a save of these blocks must contain: those of the blocks and of every separator. */
export function expectedLines(map: BlockMap, blocks: readonly string[]): string[] {
  const separators = map.seps.flatMap((sep, i) => contentLines(i === 0 ? withoutBom(sep) : sep));
  return [...separators, ...blocks.flatMap(contentLines)].sort();
}

export function spread(count: number, limit: number): number[] {
  const step = Math.max(1, Math.ceil(count / limit));
  return Array.from({ length: Math.ceil(count / step) }, (_, i) => i * step);
}

export type Splitter = {
  name: string;
  docs: Doc[];
  parse: (doc: Doc) => ParsedBlocks;
  /** The parser can read a saved text again (the mock can, the recorded ranges cannot). */
  reparse: boolean;
};

export const splitters: Splitter[] = [
  { name: "mock splitter", docs: everything, parse: (d) => mockParseBlocks(d.text), reparse: true },
  {
    name: "real ranges from Rust",
    docs: everything.filter(isCurrent),
    parse: realRanges,
    reparse: false,
  },
];
