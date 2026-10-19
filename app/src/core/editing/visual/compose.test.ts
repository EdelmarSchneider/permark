// @vitest-environment jsdom
import "./jsdomShims";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import type { Segment } from "../../../api/generated/Segment";
import { buildDoc, compose, type Built } from "./compose";
import { createCore, LOCKED, type Core } from "./core";

type Fixture = Record<string, { text: string; segments: Segment[] }>;

const corpus = JSON.parse(
  readFileSync(join(process.cwd(), "testdata/edit-corpus/segments.json"), "utf8"),
) as Fixture;

let core: Core;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.append(host);
  core = await createCore({ host });
});

afterAll(async () => {
  await core.destroy();
});

/** Block map of a small text the way the backend cuts it: blank-line separated runs (test texts only). */
function segmentsOf(text: string): Segment[] {
  const lines = text.split("\n");
  const out: Segment[] = [];
  let start = -1;
  lines.forEach((line, i) => {
    const filled = line.trim() !== "";
    if (filled && start < 0) start = i;
    if (!filled && start >= 0) {
      out.push({ lineStart: start + 1, lineEnd: i, kind: "paragraph", lock: null });
      start = -1;
    }
  });
  if (start >= 0)
    out.push({ lineStart: start + 1, lineEnd: lines.length, kind: "paragraph", lock: null });
  return out;
}

async function load(text: string, segments = segmentsOf(text)): Promise<Built> {
  const built = await buildDoc(core, text, segments);
  core.setDoc(built.doc);
  return built;
}

/** Replaces the text of the n-th top-level node (a paragraph | heading) with `text`. */
function retype(n: number, text: string): PMNode {
  const view = core.view;
  let pos = 0;
  let target: PMNode | null = null;
  view.state.doc.forEach((node, offset, index) => {
    if (index === n) {
      pos = offset;
      target = node;
    }
  });
  if (target === null) throw new Error("no node");
  const node: PMNode = target;
  const tr = view.state.tr.insertText(text, pos + 1, pos + node.nodeSize - 1);
  view.dispatch(tr);
  return view.state.doc;
}

describe("compose", () => {
  it("gives every corpus document back untouched, byte for byte", async () => {
    let documents = 0;
    let blocks = 0;
    for (const [name, { text, segments }] of Object.entries(corpus)) {
      const built = await load(text, segments);
      const composed = compose(core, built, core.view.state.doc);
      expect(composed.text, name).toBe(text);
      expect(composed.changed, name).toBe(0);
      documents += 1;
      blocks += segments.length;
    }
    expect(documents).toBeGreaterThanOrEqual(74);
    expect(blocks).toBeGreaterThan(1000);
  });

  it("changes only the edited block of every corpus document (first editable paragraph)", async () => {
    let edited = 0;
    for (const [name, { text, segments }] of Object.entries(corpus)) {
      const built = await load(text, segments);
      let index = -1;
      core.view.state.doc.forEach((node, _, i) => {
        if (index < 0 && node.type.name === "paragraph" && node.textContent.length > 0) index = i;
      });
      if (index < 0) continue;
      const source = built.sources[index];
      if (!source) continue;
      const doc = retype(index, "Changed words here");
      const composed = compose(core, built, doc);
      const before = text.slice(0, source.start);
      const after = text.slice(source.end);
      expect(composed.text.startsWith(before), name).toBe(true);
      expect(composed.text.endsWith(after), name).toBe(true);
      // the typed text takes the marks at the start of the paragraph (bold stays bold)
      const middle = composed.text.slice(before.length, composed.text.length - after.length);
      expect(middle.replace(/[*_`]/g, ""), name).toBe("Changed words here");
      expect(composed.changed, name).toBe(1);
      edited += 1;
    }
    expect(edited).toBeGreaterThan(40);
  });

  it("locks what it must not rewrite and keeps it", async () => {
    const text = "Inline $x_1$ math\n\nA [ref][r] link\n\n[r]: http://x\n\nplain *one*\n";
    const built = await load(text, [
      { lineStart: 1, lineEnd: 1, kind: "paragraph", lock: null },
      { lineStart: 3, lineEnd: 3, kind: "paragraph", lock: null },
      { lineStart: 5, lineEnd: 5, kind: "raw", lock: "definition" },
      { lineStart: 7, lineEnd: 7, kind: "paragraph", lock: null },
    ]);
    expect(built.sources.map((s) => s.locked)).toEqual(["math", "reference", "definition", null]);
    const types: string[] = [];
    core.view.state.doc.forEach((n) => types.push(n.type.name));
    expect(types).toEqual([LOCKED, LOCKED, LOCKED, "paragraph"]);
    expect(compose(core, built, retype(3, "two")).text).toBe(
      "Inline $x_1$ math\n\nA [ref][r] link\n\n[r]: http://x\n\ntwo\n",
    );
  });

  it("writes an edited block in the editor's style and keeps the blank lines around untouched blocks", async () => {
    const text = "# Title\n\n\n\nfirst  para\n\n* a\n* b\n";
    const built = await load(text);
    expect(compose(core, built, retype(1, "edited")).text).toBe(
      "# Title\n\n\n\nedited\n\n* a\n* b\n",
    );
  });

  it("undoing back to the original is no change; typing and deleting is no change", async () => {
    const text = "one\n\ntwo\n";
    const built = await load(text);
    retype(0, "uno");
    const doc = retype(0, "one");
    const composed = compose(core, built, doc);
    expect(composed.text).toBe(text);
    expect(composed.changed).toBe(0);
  });

  it("deleting a block drops its bytes and keeps its neighbours'", async () => {
    const text = "a\n\n\nb\n\nc\n";
    const built = await load(text);
    const view = core.view;
    let from = 0;
    let to = 0;
    view.state.doc.forEach((node, offset, i) => {
      if (i === 1) {
        from = offset;
        to = offset + node.nodeSize;
      }
    });
    view.dispatch(view.state.tr.delete(from, to));
    const composed = compose(core, built, view.state.doc);
    expect(composed.text).toBe("a\n\nc\n");
    expect(composed.changed).toBe(1);
  });

  it("a paragraph turned into a heading keeps the blank lines around it", async () => {
    const text = "a\n\n\n\nb\n\n\n\nc\n";
    const built = await load(text);
    const view = core.view;
    const heading = view.state.schema.nodes.heading;
    if (!heading) throw new Error("schema");
    let pos = 0;
    view.state.doc.forEach((_, offset, i) => {
      if (i === 1) pos = offset;
    });
    view.dispatch(view.state.tr.setBlockType(pos + 1, pos + 2, heading, { level: 2 }));
    expect(compose(core, built, view.state.doc).text).toBe("a\n\n\n\n## b\n\n\n\nc\n");
  });

  it("keeps two lists apart when a list is written next to one with the same marker", async () => {
    const text = "- a\n- b\n\n* c\n* d\n";
    const built = await load(text, [
      { lineStart: 1, lineEnd: 2, kind: "list", lock: null },
      { lineStart: 4, lineEnd: 5, kind: "list", lock: null },
    ]);
    const view = core.view;
    // edit the second list: the editor writes `-`, the first list has it
    let pos = 0;
    view.state.doc.forEach((node, offset, i) => {
      if (i === 1) {
        node.descendants((child, at) => {
          if (child.isText && child.text === "c") pos = offset + 1 + at;
        });
      }
    });
    view.dispatch(view.state.tr.insertText("C", pos, pos + 1));
    const out = compose(core, built, view.state.doc).text;
    expect(out.startsWith("- a\n- b\n\n")).toBe(true);
    expect(out).toMatch(/\n\n[*+] C\n[*+] d\n$/);
  });
});
