// @vitest-environment jsdom
// The round-trip safety net with the real Milkdown editor (the block model has its own in `../corpus.test.ts`). Every document
// of the corpus (the frozen snapshot of the project's own documents, the mock fixtures, the hostile set, each also as CRLF with a
// BOM; with the mock splitter and with the block ranges of the real parser) is loaded into Visual mode and:
//  - saved without an edit, it is the same bytes, and nothing shows the unsaved bar;
//  - edited in one block, only that block's bytes change;
//  - with blocks deleted, inserted or moved, no line of any separator is lost.
// A shortfall in how many blocks stay editable is a failure too: an editor that locked everything would pass the rest.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { TextSelection } from "@milkdown/kit/prose/state";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mockParseBlocks } from "../../mock/mockBlocks";
import type { BlockKind } from "../../types";
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
import { endOfText, startOf, startOfText } from "./docPositions";
import "./jsdomShims";
import { VisualEditor } from "./editor";
import type { CommandId } from "./format";
import { kindOfNode, meaningOf, splitKinds } from "./treeChecks";

const MAX_EDITS_PER_DOC = 30;
const MAX_STRUCTURAL_EDITS = 10;
const MAX_CONVERSIONS = 4;
/** Of those edits, the ones whose saved text is read again by the parser (the long documents take a while to read). */
const MAX_READ_AGAIN = 8;
/** Reading the meaning of a text takes time on the long documents. */
const SLOW = 60_000;
/** The conversions that give one block back as one block of another kind (or as a few of them). */
const CONVERSIONS: CommandId[] = ["paragraph", "h2", "quote", "bullet", "codeBlock"];

/** Neighbouring lists as one: the splitter has no way to tell two lists apart by their marker (the parser checks that, in the tests of lists). */
function oneList(kinds: BlockKind[]): BlockKind[] {
  return kinds.filter((kind, k) => !(kind === "list" && kinds[k - 1] === "list"));
}

/** A text with the marker of its lists read as one (`-`, and `1.`), for comparing what the editor wrote with what it would have by default. */
function withoutListMarkers(text: string): string {
  return text.replace(/^[-*+] /gm, "- ").replace(/^(\d{1,9})[.)] /gm, "$1. ");
}

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
      link: () => {},
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

      it(
        "changes only the bytes of an edited block, and what the saved text means stays what it meant",
        () => {
          const report = editor.load({ map });
          const starts = blockStarts(map);
          const meaning = meaningOf(editor.core, text);
          const kinds = splitKinds(text);
          const readAgain = new Set(spread(map.blocks.length, MAX_READ_AGAIN));
          for (const i of spread(map.blocks.length, MAX_EDITS_PER_DOC)) {
            if (report.reasons.has(i)) continue;
            editor.restart();
            const { view } = editor.core;
            const end = endOfText(view.state.doc, i);
            if (end === null) continue; // a rule has no text to edit
            view.dispatch(view.state.tr.insertText(" EDITED", end));
            const rewritten = tidyBlockText(
              writeNode(editor.core, view.state.doc.child(i)),
              map.eol,
            );
            expect(rewritten, `block ${i} keeps the typed text`).toContain("EDITED");
            const saved = editor.text();
            // the bytes around the block are the ones of the file...
            const head = text.slice(0, starts[i]);
            const tail = text.slice(starts[i] + map.blocks[i].text.length);
            expect(saved.startsWith(head), `block ${i}: the text before it`).toBe(true);
            expect(saved.endsWith(tail), `block ${i}: the text after it`).toBe(true);
            // ...and the block is what the editor writes, apart from the marker of a list that has to differ from its neighbour's
            const written = saved.slice(head.length, saved.length - tail.length);
            expect(withoutListMarkers(written), `block ${i}`).toBe(withoutListMarkers(rewritten));
            expect(editor.status(), `block ${i}`).toEqual({ dirty: true, changed: 1 });
            if (!readAgain.has(i)) continue;
            // what the text means: no definition is gone, and the parser finds the same blocks (not merged, not split)
            const where = `block ${i} (${map.blocks[i].kind}) of ${JSON.stringify(saved.slice(Math.max(0, head.length - 20), head.length + written.length + 20))}`;
            const now = meaningOf(editor.core, saved);
            expect(
              meaning.definitions.filter((d) => !now.definitions.includes(d)),
              `the definitions lost by an edit of ${where}`,
            ).toEqual([]);
            expect(now.blocks, `the blocks after an edit of ${where}`).toEqual(meaning.blocks);
            if (splitter.reparse) {
              // the same blocks in the same order; the edited one may be another kind (the splitter calls indented code a paragraph)
              const found = splitKinds(saved);
              expect(found.length, `the number of blocks after an edit of ${where}`).toBe(
                kinds.length,
              );
              expect(
                found.filter((_, k) => k !== i),
                `the other blocks after an edit of ${where}`,
              ).toEqual(kinds.filter((_, k) => k !== i));
            }
          }
        },
        SLOW,
      );

      if (splitter.reparse) {
        it(
          "keeps the blocks apart when a block changes kind",
          () => {
            const report = editor.load({ map });
            const definitions = meaningOf(editor.core, text).definitions;
            for (const i of spread(map.blocks.length, MAX_CONVERSIONS)) {
              if (report.reasons.has(i)) continue;
              for (const command of CONVERSIONS) {
                editor.restart();
                const { view } = editor.core;
                const at = startOfText(view.state.doc, i);
                if (at === null) continue;
                // the nodes as loaded, to tell the ones the conversion left alone from the ones it made
                const loaded = new Map<number, PMNode>();
                view.state.doc.forEach((node) => {
                  if (typeof node.attrs.tmBlock === "number") loaded.set(node.attrs.tmBlock, node);
                });
                view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, at)));
                if (!editor.run(command)) continue;
                // the blocks the document has now: the splitter's own kind for one that was not touched, the kind of its node for the rest
                const expected: (BlockKind | null)[] = [];
                view.state.doc.forEach((node) => {
                  if (node.type.name === "paragraph" && node.content.size === 0) return;
                  const origin = node.attrs.tmBlock as number | null;
                  expected.push(
                    origin !== null && loaded.get(origin)?.eq(node)
                      ? map.blocks[origin].kind
                      : kindOfNode(node),
                  );
                });
                if (expected.includes(null)) continue;
                const saved = editor.text();
                const where = `${command} on block ${i} (${map.blocks[i].kind}) of ${JSON.stringify(saved.slice(0, 300))}`;
                expect(oneList(splitKinds(saved)), `the blocks after ${where}`).toEqual(
                  oneList(expected as BlockKind[]),
                );
                if (definitions.length > 0) {
                  expect(
                    definitions.filter(
                      (d) => !meaningOf(editor.core, saved).definitions.includes(d),
                    ),
                    `the definitions lost by ${where}`,
                  ).toEqual([]);
                }
              }
            }
          },
          SLOW,
        );
      }

      it("loses no text between the blocks when blocks are deleted, inserted or moved", () => {
        const report = editor.load({ map });
        const texts = map.blocks.map((b) => b.text);
        // A list that ends up next to a list of its marker is written with another marker (or the two would be one list in the
        // file, while the view shows two), so the markers are not part of what must not be lost.
        const check = (kept: string[], label: string): void => {
          expect(contentLines(withoutListMarkers(withoutBom(editor.text()))).sort(), label).toEqual(
            expectedLines(map, kept).map(withoutListMarkers).sort(),
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
