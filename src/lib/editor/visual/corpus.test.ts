// @vitest-environment jsdom
// The round-trip safety net with the real Milkdown editor (the block model has its own in `../corpus.test.ts`). Every document
// of the corpus (the frozen snapshot of the project's own documents, the mock fixtures, the hostile set, each also as CRLF with a
// BOM; with the mock splitter and with the block ranges of the real parser) is loaded into Visual mode and:
//  - saved without an edit, it is the same bytes, and nothing shows the unsaved bar;
//  - edited in one block, only that block's bytes change;
//  - with blocks deleted, inserted or moved, no line of any separator is lost.
// A shortfall in how many blocks stay editable is a failure too: an editor that locked everything would pass the rest.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mockParseBlocks } from "../../mock/mockBlocks";
import {
  buildBlockMap,
  changedBlockCount,
  matchBlocks,
  tidyBlockText,
  type BlockMap,
} from "../blockModel";
import {
  contentLines,
  expectedLines,
  isCurrent,
  snapshot,
  spread,
  splitters,
  withoutBom,
} from "../corpusDocs";
import { writeNode } from "./doc";
import "./jsdomShims";
import { VisualEditor } from "./editor";

const MAX_EDITS_PER_DOC = 30;
const MAX_STRUCTURAL_EDITS = 10;

let editor: VisualEditor;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.appendChild(host);
  editor = await VisualEditor.create({
    host,
    label: "corpus",
    scroll: false,
    imageUrl: (src) => src,
    locked: {
      render: (body, atom) => {
        body.textContent = atom.source;
      },
      edit: () => {},
      labels: { edit: "Edit in Source", editHint: "", region: (kind) => kind, reason: () => "" },
    },
    handlers: {
      changed: () => {},
      files: () => {},
      editInSource: () => {},
      link: () => {},
      slash: () => {},
      slashKey: () => false,
    },
  });
});
afterAll(async () => {
  await editor.destroy();
});

/** The position where a top-level node starts. */
function startOf(doc: PMNode, index: number): number {
  let pos = 0;
  for (let i = 0; i < index; i++) pos += doc.child(i).nodeSize;
  return pos;
}

/** The end of the text of the last text block inside a top-level node, or null for a node without one (a rule, an atom). */
function endOfText(doc: PMNode, index: number): number | null {
  const start = startOf(doc, index);
  const child = doc.child(index);
  if (child.isTextblock) return start + 1 + child.content.size;
  let found: number | null = null;
  child.descendants((node, pos) => {
    if (node.isTextblock) found = start + 1 + pos + 1 + node.content.size;
  });
  return found;
}

/** Where each block starts in the text. */
function blockStarts(map: BlockMap): number[] {
  let at = map.seps[0].length;
  return map.blocks.map((block, i) => {
    const start = at;
    at += block.text.length + map.seps[i + 1].length;
    return start;
  });
}

const totals = { blocks: 0, editable: 0 };

for (const splitter of splitters.filter((s) => s.docs.length > 0)) {
  describe(`Visual mode with the ${splitter.name}`, () => {
    describe.each(splitter.docs.map((d) => [d.name, d] as const))("%s", (_name, doc) => {
      const { text } = doc;
      const map = buildBlockMap(text, splitter.parse(doc));

      it("loads without an edit and saves as the same bytes, with nothing marked unsaved", () => {
        const report = editor.load({ map });
        expect(editor.text()).toBe(text);
        expect(editor.status()).toEqual({ dirty: false, changed: 0 });
        expect(report.editable + Object.values(report.locked).reduce((a, b) => a + b, 0)).toBe(
          map.blocks.length,
        );
        if (splitter.name.startsWith("real") && snapshot.some((d) => d.name === doc.name)) {
          totals.blocks += map.blocks.length;
          totals.editable += report.editable;
        }
      });

      it("changes only the bytes of an edited block", () => {
        const report = editor.load({ map });
        const starts = blockStarts(map);
        for (const i of spread(map.blocks.length, MAX_EDITS_PER_DOC)) {
          if (report.reasons.has(i)) continue;
          editor.restart();
          const { view } = editor.core;
          const end = endOfText(view.state.doc, i);
          if (end === null) continue; // a rule has no text to edit
          view.dispatch(view.state.tr.insertText(" EDITED", end));
          const rewritten = tidyBlockText(writeNode(editor.core, view.state.doc.child(i)), map.eol);
          expect(rewritten, `block ${i} keeps the typed text`).toContain("EDITED");
          const expected =
            text.slice(0, starts[i]) +
            rewritten +
            text.slice(starts[i] + map.blocks[i].text.length);
          expect(editor.text(), `block ${i}`).toBe(expected);
          expect(editor.status(), `block ${i}`).toEqual({ dirty: true, changed: 1 });
        }
      });

      it("loses no text between the blocks when blocks are deleted, inserted or moved", () => {
        const report = editor.load({ map });
        const texts = map.blocks.map((b) => b.text);
        const check = (kept: string[], label: string): void => {
          expect(contentLines(withoutBom(editor.text())).sort(), label).toEqual(
            expectedLines(map, kept),
          );
        };
        for (const i of spread(map.blocks.length, MAX_STRUCTURAL_EDITS)) {
          const { view } = editor.core;
          const reload = (): void => editor.restart();

          reload();
          let doc = view.state.doc;
          view.dispatch(view.state.tr.delete(startOf(doc, i), startOf(doc, i + 1)));
          check(
            texts.filter((_, k) => k !== i),
            `delete ${i}`,
          );

          reload();
          doc = view.state.doc;
          view.dispatch(
            view.state.tr.insert(
              startOf(doc, i),
              view.state.schema.nodes.paragraph.create(null, view.state.schema.text("NEW")),
            ),
          );
          check([...texts, "NEW"], `insert at ${i}`);

          reload();
          doc = view.state.doc;
          const node = doc.child(i);
          const tr = view.state.tr.delete(startOf(doc, i), startOf(doc, i + 1));
          view.dispatch(tr.insert(tr.doc.content.size, node));
          check([...texts.filter((_, k) => k !== i), texts[i]], `move ${i} to the end`);
        }
        expect(report.editable).toBeGreaterThanOrEqual(0);
      });

      if (splitter.reparse) {
        it("shows the blocks that differ from the saved file after a switch from Source", () => {
          const index = map.blocks.findIndex((b) => b.kind === "paragraph");
          if (index < 0) return;
          // Source edits one paragraph; Visual loads that text with the saved file's blocks as the reference
          const working = text.replace(
            map.blocks[index].text,
            `${map.blocks[index].text} edited in Source`,
          );
          if (working === text) return;
          const next = buildBlockMap(working, mockParseBlocks(working));
          const match = matchBlocks(map, next);
          editor.load({
            map: next,
            unsaved: match.origins.map((origin) => origin === null),
            dirty: working !== text,
          });
          expect(editor.text()).toBe(working);
          expect(editor.status().dirty).toBe(true);
          expect(editor.status().changed).toBe(changedBlockCount(match));
        });
      }
    });
  });
}

describe("the share of blocks Visual mode can edit", () => {
  it("is high for the project's own documents (an editor that locks everything is no editor)", () => {
    const real = splitters.find((s) => s.name.startsWith("real"));
    if (!real) return;
    // read after the tests above have run
    const documents = snapshot.filter(isCurrent).length;
    expect(documents).toBeGreaterThanOrEqual(5);
    expect(totals.blocks).toBeGreaterThan(0);
    expect(totals.editable / totals.blocks).toBeGreaterThan(0.85);
  });
});
