// The round-trip safety net of the editor: for every Markdown document in a frozen snapshot of the project's own
// documents, in the mock fixtures and in a set of hostile ones, loading it into the block model and saving without an edit must give
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
// The project's documents are a frozen snapshot in `corpus/docs/` (the README, the changelog, the contributing
// guide and the design notes, with names like `README.md` and `design/v0.2.md`). Nothing here reads the live files,
// so editing them never makes the fixtures stale, and the corpus is the same on every checkout. Only a change to the
// parser, or to the mock and hostile sets, does. To refresh the snapshot on purpose, copy the current files over it
// and regenerate the fixtures with the two commands above:
//
//   cp README.md CHANGELOG.md CONTRIBUTING.md src/lib/editor/corpus/docs/
//   cp docs/design/*.md src/lib/editor/corpus/docs/design/

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { describe, expect, it } from "vitest";
import { mockParseBlocks } from "../mock/mockBlocks";
import { mockSources } from "../mock/mockSource";
import {
  BOM,
  buildBlockMap,
  changedBlockCount,
  compose,
  joinBlockMap,
  matchBlocks,
  tidyBlockText,
  untouchedSlots,
  type Slot,
} from "./blockModel";
import { fromBuffer, toBuffer } from "./bufferText";
import {
  GENERATED,
  INLINE,
  RANGES,
  REGENERATE,
  UPDATE,
  contentLines,
  everything,
  expectedLines,
  inline,
  isCurrent,
  snapshot,
  spread,
  splitters,
  withoutBom,
} from "./corpusDocs";
import { HOSTILE } from "./hostileFixtures";

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

describe("the corpus", () => {
  it("covers the snapshot of the project's documents, the mock fixtures and the hostile set", () => {
    expect(snapshot.map((d) => d.name)).toEqual(
      expect.arrayContaining(["README.md", "CHANGELOG.md", "CONTRIBUTING.md"]),
    );
    expect(snapshot.length).toBeGreaterThanOrEqual(5);
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
