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

/** Replaces the first `find` in a text node of the document with `by`. */
function replaceText(find: string, by: string): PMNode {
  const view = core.view;
  let at = -1;
  view.state.doc.descendants((node, pos) => {
    if (at >= 0) return false;
    const i = node.isText ? (node.text ?? "").indexOf(find) : -1;
    if (i >= 0) at = pos + i;
    return true;
  });
  if (at < 0) throw new Error(`no ${find}`);
  view.dispatch(view.state.tr.insertText(by, at, at + find.length));
  return view.state.doc;
}

/** Types `text` at the end of the last text block of the n-th top-level node. */
function typeAtEnd(n: number, text: string): PMNode {
  const view = core.view;
  let at = -1;
  view.state.doc.forEach((node, offset, index) => {
    if (index !== n) return;
    if (node.isTextblock) at = offset + node.nodeSize - 1;
    node.descendants((child, pos) => {
      if (child.isTextblock) at = offset + 1 + pos + 1 + child.content.size;
      return true;
    });
  });
  if (at < 0) throw new Error("no text block");
  view.dispatch(view.state.tr.insertText(text, at));
  return view.state.doc;
}

/** Every corpus document is built in a real editor: slow machines need more than the default 5 s. */
const CORPUS_MS = 30_000;

describe("compose", () => {
  it(
    "gives every corpus document back untouched, byte for byte",
    async () => {
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
    },
    CORPUS_MS,
  );

  it(
    "changes only the edited block of every corpus document (first editable paragraph)",
    async () => {
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
    },
    CORPUS_MS,
  );

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

  describe("adjacent edited blocks keep the separator between them", () => {
    it("two paragraphs", async () => {
      const built = await load("a teh\n\n\n\nb teh\n\nc\n");
      replaceText("a teh", "a the");
      const composed = compose(core, built, replaceText("b teh", "b the"));
      expect(composed.text).toBe("a the\n\n\n\nb the\n\nc\n");
      expect(composed.changed).toBe(2);
    });

    it("a list and a table", async () => {
      const text = "Intro\n\n- one teh\n- two\n\n\n\n| a | b |\n| - | - |\n| teh | x |\n\nEnd\n";
      const built = await load(text);
      replaceText("one teh", "one the");
      const composed = compose(core, built, replaceText("teh", "the"));
      expect(composed.text.startsWith("Intro\n\n")).toBe(true);
      expect(composed.text.endsWith("\n\nEnd\n")).toBe(true);
      expect(composed.text).toMatch(/one the\n[-*+] two\n\n\n\n\|/);
      expect(composed.text).not.toContain("teh");
      expect(composed.changed).toBe(2);
    });

    it("a front matter card and the block after it", async () => {
      const text = "---\ntitle: teh\n---\n\n\n\nBody teh.\n\nEnd\n";
      const built = await load(text, [
        { lineStart: 1, lineEnd: 3, kind: "frontMatter", lock: "frontMatter" },
        { lineStart: 7, lineEnd: 7, kind: "paragraph", lock: null },
        { lineStart: 9, lineEnd: 9, kind: "paragraph", lock: null },
      ]);
      const view = core.view;
      const card = view.state.doc.firstChild;
      if (card === null) throw new Error("no card");
      view.dispatch(
        view.state.tr.setNodeMarkup(0, undefined, {
          ...card.attrs,
          source: "---\ntitle: the\n---",
        }),
      );
      const composed = compose(core, built, replaceText("Body teh.", "Body the."));
      expect(composed.text).toBe("---\ntitle: the\n---\n\n\n\nBody the.\n\nEnd\n");
      expect(composed.changed).toBe(2);
    });
  });

  describe("a different number of blocks: separators fall back as before", () => {
    const text = "a\n\n\n\nb c\n\n\n\nd\n";

    it("one block split into two", async () => {
      const built = await load(text);
      const view = core.view;
      let at = -1;
      view.state.doc.descendants((node, pos) => {
        if (node.isText && node.text === "b c") at = pos;
      });
      // "b c" -> "b" | "c"
      view.dispatch(view.state.tr.split(at + 1).delete(at + 3, at + 4));
      const composed = compose(core, built, view.state.doc);
      expect(composed.text).toBe("a\n\nb\n\nc\n\nd\n");
      expect(composed.changed).toBe(3);
    });

    it("two blocks merged", async () => {
      const built = await load(text);
      const view = core.view;
      const second = view.state.doc.child(0).nodeSize + view.state.doc.child(1).nodeSize;
      view.dispatch(view.state.tr.join(second));
      const composed = compose(core, built, view.state.doc);
      expect(composed.text).toBe("a\n\nb cd\n");
      expect(composed.changed).toBe(3);
    });

    it("a block deleted and its neighbour edited", async () => {
      const built = await load(text);
      const view = core.view;
      const first = view.state.doc.child(0).nodeSize;
      view.dispatch(view.state.tr.delete(first, first + view.state.doc.child(1).nodeSize));
      const composed = compose(core, built, replaceText("d", "D"));
      expect(composed.text).toBe("a\n\nD\n");
      expect(composed.changed).toBe(3);
    });
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

  describe("an edited block keeps every character the person did not change", () => {
    const list = (lines: number): Segment[] => [
      { lineStart: 1, lineEnd: lines, kind: "list", lock: null },
    ];
    // each block is written differently by the editor than in the file; one letter typed at its end
    const cases: { name: string; text: string; segments?: Segment[]; expected: string }[] = [
      {
        name: "a bare web address",
        text: "Lorem ipsum (https://example.com/a/b) dolor\n",
        expected: "Lorem ipsum (https://example.com/a/b) dolorZ\n",
      },
      {
        name: "a dot after a word",
        text: "Lorem low..high dolor amet below.\n",
        expected: "Lorem low..high dolor amet below.Z\n",
      },
      {
        name: "a tilde",
        text: "Lorem ipsum ~100 ms amet\n",
        expected: "Lorem ipsum ~100 ms ametZ\n",
      },
      {
        name: "a bracket",
        text: "Lorem [B] ipsum amet\n",
        expected: "Lorem [B] ipsum ametZ\n",
      },
      {
        name: "a bracket in a heading",
        text: "## [0.5.7] - 2026-01-01\n",
        expected: "## [0.5.7] - 2026-01-01Z\n",
      },
      {
        name: "a star that marks nothing",
        text: "Lorem ipsum-* dolor amet\n",
        expected: "Lorem ipsum-* dolor ametZ\n",
      },
      {
        name: "a loose list with one blank line",
        text: "- lorem\n- ipsum\n\n- dolor\n",
        segments: list(4),
        expected: "- lorem\n- ipsum\n\n- dolorZ\n",
      },
      {
        name: "an escaped pipe outside a table",
        text: "Lorem (ipsum \\| dolor) amet\n",
        expected: "Lorem (ipsum \\| dolor) ametZ\n",
      },
      {
        name: "an indented paragraph",
        text: "   Lorem ipsum dolor amet\n",
        expected: "   Lorem ipsum dolor ametZ\n",
      },
      {
        name: "a lazy continuation line",
        text: "- lorem ipsum\nDolor sit amet\n",
        segments: list(2),
        expected: "- lorem ipsum\nDolor sit ametZ\n",
      },
      {
        name: "star bullets",
        text: "* lorem\n* ipsum\n",
        segments: list(2),
        expected: "* lorem\n* ipsumZ\n",
      },
      {
        name: "a table in a list",
        text: "- lorem\n\n  | a | b |\n  |---|---|\n  | lorem ipsum | c |\n",
        segments: list(5),
        expected: "- lorem\n\n  | a | b |\n  |---|---|\n  | lorem ipsum | cZ |\n",
      },
      {
        name: "a padded code span",
        text: "Lorem ` or ` ipsum\n",
        expected: "Lorem ` or ` ipsumZ\n",
      },
      {
        name: "a trailing space",
        text: "Lorem ipsum. \n\nDolor\n",
        expected: "Lorem ipsum.Z \n\nDolor\n",
      },
    ];
    for (const { name, text, segments, expected } of cases) {
      it(name, async () => {
        const built = await load(text, segments);
        const composed = compose(core, built, typeAtEnd(0, "Z"));
        expect(composed.text).toBe(expected);
        expect(composed.changed).toBe(1);
      });
    }

    it("typing at the start of an indented paragraph keeps the indentation before it", async () => {
      const built = await load("   Lorem ipsum\n");
      const view = core.view;
      view.dispatch(view.state.tr.insertText("Z", 1));
      expect(compose(core, built, view.state.doc).text).toBe("   ZLorem ipsum\n");
    });

    it("deleting an escaped character deletes its escape too", async () => {
      const built = await load("Lorem [B] (ipsum \\| dolor) amet\n");
      expect(compose(core, built, replaceText("|", "")).text).toBe(
        "Lorem [B] (ipsum  dolor) amet\n",
      );
    });
  });
});
