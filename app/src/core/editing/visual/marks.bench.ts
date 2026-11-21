// @vitest-environment jsdom
// Cost of Edit-mode marks and of the reference check behind each accepted edit on large documents. Not part of the
// test run: `npx vitest bench --run --reporter=verbose src/core/editing/visual/marks.bench.ts`.
import "./jsdomShims";
import { $prose } from "@milkdown/kit/utils";
import { beforeAll, describe, test } from "vitest";
import {
  misspelledRanges,
  paragraphSegments,
  paragraphsText,
  tableText,
} from "../../testing/editDocs";
import type { TextRange } from "../session.svelte";
import { blocksUsingReferences } from "./classify";
import { buildDoc, compose, type Composed } from "./compose";
import { createCore, type Core } from "./core";
import { changeable, docRange, docRanges } from "./edits";
import { marksPlugin, setMarks } from "./marks";

/** Few rounds: one round of the slow cases takes seconds. */
const FEW = { iterations: 5, time: 0, warmupIterations: 1, warmupTime: 0 };

let core: Core;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.append(host);
  core = await createCore({ host, plugins: [$prose(() => marksPlugin())] });
});

async function load(text: string): Promise<Composed> {
  const built = await buildDoc(core, text, paragraphSegments(text));
  core.setDoc(built.doc);
  return compose(core, built, core.view.state.doc);
}

describe("Edit marks", () => {
  test("8000 paragraphs", async ({ bench }) => {
    const text = paragraphsText(8000);
    const composed = await load(text);
    const ranges = misspelledRanges(text).slice(0, 30_000);
    const last: TextRange = ranges[ranges.length - 1] ?? { from: 0, to: 0 };
    await bench("docRange: one range in the last block", () => {
      docRange(core.view.state.doc, composed, last);
    }).run();
    await bench("docRanges: 30000 ranges", () => {
      docRanges(core.view.state.doc, composed, ranges);
    }).run(FEW);
    await bench("setMarks: 30000 ranges", () => {
      setMarks(core.view, composed, ranges, null, () => {});
    }).run(FEW);
  });

  test("one 160 KB table", async ({ bench }) => {
    const text = tableText(3200);
    const composed = await load(text);
    const ranges = misspelledRanges(text, 4).slice(0, 2000);
    await bench("docRanges: 2000 ranges in the table", () => {
      docRanges(core.view.state.doc, composed, ranges);
    }).run(FEW);
    await bench("setMarks: 2000 ranges in the table", () => {
      setMarks(core.view, composed, ranges, null, () => {});
    }).run(FEW);
  });
});

describe("accepted fix", () => {
  test("5000 paragraphs and a footnote", async ({ bench }) => {
    const text = `${paragraphsText(5000)}\nA note[^a] here.\n\n[^a]: The note.\n`;
    const composed = await load(text);
    const at = composed.text.lastIndexOf("teh");
    await bench("changeable: one word fixed", () => {
      changeable(core, composed, { from: at, to: at + 3 }, "the");
    }).run(FEW);
  });
});

describe("reference check", () => {
  test("1 MB with footnotes", async ({ bench }) => {
    const blocks: string[] = [];
    let size = 0;
    for (let i = 0; size < 1_000_000; i++) {
      const block = `Paragraph ${i} cites a note[^n${i % 50}] and goes on with plain words here.`;
      blocks.push(block);
      size += block.length + 2;
    }
    for (let n = 0; n < 50; n++) blocks.push(`[^n${n}]: Note ${n}.`);
    const text = blocks.join("\n\n");
    const starts: number[] = [];
    let at = 0;
    for (const block of blocks) {
      starts.push(at);
      at += block.length + 2;
    }
    await bench("blocksUsingReferences", () => {
      blocksUsingReferences(core, text, starts);
    }).run(FEW);
  });
});
