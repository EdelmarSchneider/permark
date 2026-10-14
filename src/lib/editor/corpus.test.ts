// The round-trip safety net of the editor: for every Markdown document in this repository's public files, in the
// mock fixtures and in a set of hostile ones, loading it into the block model and saving without an edit must give
// back the very same bytes, and editing one block must change that block's bytes and nothing else. Deleting,
// inserting and moving blocks must not lose any text that lives between them (link reference definitions).
//
// Every check runs twice. Once with the mock splitter (`mock/mockBlocks.ts`, a line-based approximation of
// CommonMark that lets the tests re-parse a saved text), and once with the block ranges the real parser found:
// vitest cannot call Rust, so a Rust test (`src-tauri/src/edit/corpus.rs`) parses the same documents and keeps the
// result in `corpus/ranges.json`, which is read here. `corpus/inline.json` carries the texts that live in
// TypeScript to that Rust test. Both files are checked for staleness; to regenerate them:
//
//   UPDATE_CORPUS=1 npx vitest run src/lib/editor/corpus.test.ts
//   UPDATE_CORPUS=1 cargo test --manifest-path src-tauri/Cargo.toml corpus
//
// Only public files are in the corpus (the root documents, `docs/design/`, the mock and hostile sets), so it is
// the same on every checkout.

import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, expect, it } from "vitest";
import { mockParseBlocks } from "../mock/mockBlocks";
import { mockSources } from "../mock/mockSource";
import type { BlockKind, ParsedBlocks } from "../types";
import {
  BOM,
  buildBlockMap,
  changedBlockCount,
  compose,
  joinBlockMap,
  matchBlocks,
  tidyBlockText,
  untouchedSlots,
  type BlockMap,
  type Slot,
} from "./blockModel";
import { fromBuffer, toBuffer } from "./bufferText";
import { HOSTILE } from "./hostileFixtures";

type Doc = { name: string; text: string };

const UPDATE = process.env.UPDATE_CORPUS === "1";
const INLINE = "src/lib/editor/corpus/inline.json";
const RANGES = "src/lib/editor/corpus/ranges.json";
const REGENERATE =
  "regenerate with: UPDATE_CORPUS=1 npx vitest run src/lib/editor/corpus.test.ts && " +
  "UPDATE_CORPUS=1 cargo test --manifest-path src-tauri/Cargo.toml corpus";
/** Built from repeated text at run time; they are about size, which the Rust tests cover, and would only bloat the fixtures. */
const GENERATED = new Set(["hostile: very long lines", "hostile: many tiny blocks"]);

function markdownFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name).replace(/\\/g, "/");
    if (entry.isDirectory()) out.push(...markdownFiles(path));
    else if (entry.name.endsWith(".md")) out.push(path);
  }
  return out;
}

function repositoryDocs(): Doc[] {
  const files = [...markdownFiles("docs/design"), "README.md", "CHANGELOG.md", "CONTRIBUTING.md"];
  return files.sort().map((name) => ({ name, text: readFileSync(name, "utf8") }));
}

const repository = repositoryDocs();
const inline: Doc[] = [
  ...Object.entries(mockSources()).map(([name, text]) => ({ name: `mock ${name}`, text })),
  ...Object.entries(HOSTILE).map(([name, text]) => ({ name: `hostile: ${name}`, text })),
];
const docs: Doc[] = [...repository, ...inline];
// the same documents as a Windows editor would have saved them
const windows: Doc[] = docs.map(({ name, text }) => ({
  name: `${name} (CRLF, BOM)`,
  text: BOM + text.replace(/\r\n|\r|\n/g, "\r\n"),
}));
const everything = [...docs, ...windows];

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
function fingerprint(text: string): number {
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
function isCurrent(doc: Doc): boolean {
  const entry = ranges.get(doc.name);
  return (
    entry !== undefined &&
    entry.bytes === encoder.encode(doc.text).length &&
    entry.fnv === fingerprint(doc.text)
  );
}

function realRanges(doc: Doc): ParsedBlocks {
  const entry = ranges.get(doc.name) as RangeEntry;
  return {
    eol: entry.eol,
    blocks: entry.blocks.map(([startByte, endByte, kind]) => ({ startByte, endByte, kind })),
  };
}

describe("the files shared with the Rust test", () => {
  it("inline.json holds the texts of the mock and hostile documents", () => {
    const shared = Object.fromEntries(
      inline.filter((d) => !GENERATED.has(d.name)).map((d) => [d.name, d.text]),
    );
    const text = `${JSON.stringify(shared, null, 1)}\n`;
    if (UPDATE) {
      mkdirSync(dirname(INLINE), { recursive: true });
      writeFileSync(INLINE, text);
    }
    expect(existsSync(INLINE), `${INLINE} is missing: ${REGENERATE}`).toBe(true);
    expect(
      readFileSync(INLINE, "utf8").replace(/\r\n/g, "\n"),
      `${INLINE} is stale: ${REGENERATE}`,
    ).toBe(text);
  });

  it("ranges.json has the real block ranges of every document as it is now", () => {
    const stale = everything.filter(
      (d) => !GENERATED.has(d.name.replace(/ \(CRLF, BOM\)$/, "")) && !isCurrent(d),
    );
    expect(
      stale.map((d) => d.name),
      `${RANGES} is stale or missing: ${REGENERATE}`,
    ).toEqual([]);
  });
});

// ---- the checks ---------------------------------------------------------------------------------------------

const MAX_EDITS_PER_DOC = 40;
const MAX_STRUCTURAL_EDITS = 12;

const withoutBom = (text: string): string => (text.startsWith(BOM) ? text.slice(BOM.length) : text);

/** The lines of a text that are not blank, without their terminators. */
function contentLines(text: string): string[] {
  return text.split(/\r\n|\r|\n/).filter((line) => line.trim() !== "");
}

/** The non-blank lines that a save of these blocks must contain: those of the blocks and of every separator. */
function expectedLines(map: BlockMap, blocks: readonly string[]): string[] {
  const separators = map.seps.flatMap((sep, i) => contentLines(i === 0 ? withoutBom(sep) : sep));
  return [...separators, ...blocks.flatMap(contentLines)].sort();
}

function spread(count: number, limit: number): number[] {
  const step = Math.max(1, Math.ceil(count / limit));
  return Array.from({ length: Math.ceil(count / step) }, (_, i) => i * step);
}

type Splitter = { name: string; docs: Doc[]; parse: (doc: Doc) => ParsedBlocks; reparse: boolean };

const splitters: Splitter[] = [
  { name: "mock splitter", docs: everything, parse: (d) => mockParseBlocks(d.text), reparse: true },
  {
    name: "real ranges from Rust",
    docs: everything.filter(isCurrent),
    parse: realRanges,
    reparse: false,
  },
];

describe("the corpus", () => {
  it("covers the repository's public documents, the mock fixtures and the hostile set", () => {
    expect(repository.map((d) => d.name)).toEqual(
      expect.arrayContaining(["README.md", "CHANGELOG.md", "CONTRIBUTING.md"]),
    );
    expect(repository.length).toBeGreaterThanOrEqual(5);
    expect(Object.keys(mockSources()).length).toBeGreaterThan(10);
    expect(Object.keys(HOSTILE).length).toBeGreaterThan(30);
  });

  describe.each(splitters.filter((s) => s.docs.length > 0))("with the $name", (splitter) => {
    describe.each(splitter.docs.map((d) => [d.name, d] as const))("%s", (_name, doc) => {
      const { text } = doc;
      const map = buildBlockMap(text, splitter.parse(doc));

      it("loads and saves without an edit as the same bytes", () => {
        expect(joinBlockMap(map)).toBe(text);
        expect(compose(map, untouchedSlots(map))).toBe(text);
      });

      it("keeps its BOM and every line ending through the editor buffer", () => {
        const { buffer, format } = toBuffer(text);
        expect(buffer).not.toContain("\r");
        const base = { text, buffer };
        // whatever the line endings are (one kind, mixed, a lone CR), a save without an edit is the same bytes
        expect(fromBuffer(buffer, format, base)).toBe(text);
        // and an edit inside a line leaves every line ending of the file alone
        const at = buffer.search(/[^\n]/);
        if (at < 0) return;
        const edited = `${buffer.slice(0, at + 1)}EDITED${buffer.slice(at + 1)}`;
        const saved = fromBuffer(edited, format, base);
        expect(toBuffer(saved).buffer).toBe(edited);
        const breaks = (s: string): string[] => s.match(/\r\n|\r|\n/g) ?? [];
        expect(breaks(saved)).toEqual(breaks(text));
      });

      it("changes only the bytes of an edited block", () => {
        let offset = map.seps[0].length;
        const starts = map.blocks.map((b, i) => {
          const at = offset;
          offset += b.text.length + map.seps[i + 1].length;
          return at;
        });
        for (const i of spread(map.blocks.length, MAX_EDITS_PER_DOC)) {
          const block = map.blocks[i].text;
          const slots = untouchedSlots(map);
          slots[i] = { origin: i, text: `${block} EDITED` };
          // an edited block gets the file's line ending (it differs from the block's own only in a mixed file)
          const edited = tidyBlockText(`${block} EDITED`, map.eol);
          const expected = `${text.slice(0, starts[i])}${edited}${text.slice(starts[i] + block.length)}`;
          expect(compose(map, slots), `block ${i}`).toBe(expected);
        }
      });

      it("loses no text between the blocks when blocks are deleted, inserted or moved", () => {
        const texts = map.blocks.map((b) => b.text);
        const check = (slots: Slot[], label: string): void => {
          const saved = compose(map, slots);
          const kept = slots.map((s) => s.text ?? texts[s.origin as number]);
          expect(contentLines(withoutBom(saved)).sort(), label).toEqual(expectedLines(map, kept));
        };
        for (const i of spread(map.blocks.length, MAX_STRUCTURAL_EDITS)) {
          const all = untouchedSlots(map);
          check(
            all.filter((_, k) => k !== i),
            `delete ${i}`,
          );
          check(
            [...all.slice(0, i), { origin: null, text: "NEW" }, ...all.slice(i)],
            `insert at ${i}`,
          );
          check([...all.filter((_, k) => k !== i), all[i]], `move ${i} to the end`);
        }
        check([{ origin: null, text: "NEW" }], "replace everything");
        check([], "delete everything");
      });

      if (splitter.reparse) {
        it("sees exactly one changed block after an edit to a paragraph", () => {
          const index = map.blocks.findIndex((b) => b.kind === "paragraph");
          if (index < 0) return;
          const slots = untouchedSlots(map);
          slots[index] = { origin: index, text: `${map.blocks[index].text} edited` };
          const saved = compose(map, slots);
          const next = buildBlockMap(saved, mockParseBlocks(saved));
          expect(changedBlockCount(matchBlocks(map, next)), "changed blocks").toBe(1);
        });
      }
    });
  });
});
