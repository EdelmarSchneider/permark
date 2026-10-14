// The round-trip safety net of the editor (docs/plan-editor.md section 4): for every Markdown document in this
// repository and for a set of hostile ones, loading it into the block model and saving without an edit must give
// back the very same bytes, and editing one block must change that block's bytes and nothing else.
//
// The ranges come from the mock splitter here, because vitest cannot call Rust; the same documents are checked
// against the real block parser by a Rust test (`edit::blocks`), which asserts that blocks and separators add up
// to the file and that nothing a reader sees hides in a separator.

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
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
} from "./blockModel";
import { fromBuffer, toBuffer } from "./bufferText";
import { HOSTILE } from "./hostileFixtures";

type Doc = { name: string; text: string };

function markdownFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...markdownFiles(path));
    else if (entry.name.endsWith(".md")) out.push(path);
  }
  return out;
}

function repositoryDocs(): Doc[] {
  const files = [...markdownFiles("docs"), "README.md", "CHANGELOG.md", "HANDOFF.md"];
  return files.map((name) => ({ name, text: readFileSync(name, "utf8") }));
}

const repository = repositoryDocs();
const docs: Doc[] = [
  ...repository,
  ...Object.entries(mockSources()).map(([name, text]) => ({ name: `mock ${name}`, text })),
  ...Object.entries(HOSTILE).map(([name, text]) => ({ name: `hostile: ${name}`, text })),
];
// the same documents as a Windows editor would have saved them
const windows: Doc[] = docs.map(({ name, text }) => ({
  name: `${name} (CRLF, BOM)`,
  text: BOM + text.replace(/\r\n|\r|\n/g, "\r\n"),
}));

const MAX_EDITS_PER_DOC = 40;

describe("the corpus", () => {
  it("covers the repository's documents, the mock fixtures and the hostile set", () => {
    expect(repository.length).toBeGreaterThan(10);
    expect(Object.keys(mockSources()).length).toBeGreaterThan(10);
    expect(Object.keys(HOSTILE).length).toBeGreaterThan(30);
  });

  describe.each([...docs, ...windows].map((d) => [d.name, d.text] as const))(
    "%s",
    (_name, text) => {
      const map = buildBlockMap(text, mockParseBlocks(text));

      it("loads and saves without an edit as the same bytes", () => {
        expect(joinBlockMap(map)).toBe(text);
        expect(compose(map, untouchedSlots(map))).toBe(text);
      });

      it("keeps its BOM and line endings through the editor buffer when it has one kind of line ending", () => {
        const { buffer, format } = toBuffer(text);
        expect(buffer).not.toContain("\r");
        const loneCr = /\r(?!\n)/.test(text);
        const mixed = text.includes("\r\n") && /(?<!\r)\n/.test(text);
        if (!loneCr && !mixed) expect(fromBuffer(buffer, format)).toBe(text);
      });

      it("changes only the bytes of an edited block", () => {
        const step = Math.max(1, Math.ceil(map.blocks.length / MAX_EDITS_PER_DOC));
        let offset = map.seps[0].length;
        const starts = map.blocks.map((b, i) => {
          const at = offset;
          offset += b.text.length + map.seps[i + 1].length;
          return at;
        });
        for (let i = 0; i < map.blocks.length; i += step) {
          const block = map.blocks[i].text;
          const slots = untouchedSlots(map);
          slots[i] = { origin: i, text: `${block} EDITED` };
          // an edited block gets the file's line ending (it differs from the block's own only in a mixed file)
          const edited = tidyBlockText(`${block} EDITED`, map.eol);
          const expected = `${text.slice(0, starts[i])}${edited}${text.slice(starts[i] + block.length)}`;
          expect(compose(map, slots), `block ${i}`).toBe(expected);
        }
      });

      it("sees exactly one changed block after an edit to a paragraph", () => {
        const index = map.blocks.findIndex((b) => b.kind === "paragraph");
        if (index < 0) return;
        const slots = untouchedSlots(map);
        slots[index] = { origin: index, text: `${map.blocks[index].text} edited` };
        const saved = compose(map, slots);
        const next = buildBlockMap(saved, mockParseBlocks(saved));
        expect(changedBlockCount(matchBlocks(map, next)), "changed blocks").toBe(1);
      });
    },
  );
});
