// @vitest-environment jsdom
import "./jsdomShims";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import type { Block } from "../../../api/generated/Block";
import type { Body } from "../../../api/generated/Body";
import type { FrontMatter } from "../../../api/generated/FrontMatter";
import type { Segment } from "../../../api/generated/Segment";
import { createMockConfig } from "../../../api/mockConfig";
import { createMockEditing } from "../../../api/mockEdit";
import { buildDoc, compose } from "./compose";
import { createCore, type Core } from "./core";
import {
  LookBook,
  lockedLooks,
  lockedRenders,
  sameLook,
  type LineBlock,
  type LockedRender,
} from "./lockedRender";

let core: Core;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.append(host);
  core = await createCore({ host });
});

afterAll(async () => {
  await core.destroy();
});

const config = createMockConfig();
const editing = createMockEditing(() => []).api;

/** A fixture document as the editor opens it (text + backend block map). */
async function fixture(path: string): Promise<{ text: string; segments: Segment[] }> {
  const result = await editing.editSource(path);
  if (!result.ok || result.value.segments === null) throw new Error(`no source for ${path}`);
  return { text: result.value.text, segments: result.value.segments };
}

/** The editor's blocks of `text` (the real build: backend locks + the editor's own). */
async function editorBlocks(text: string, segments: readonly Segment[]): Promise<LineBlock[]> {
  const built = await buildDoc(core, text, segments);
  return built.sources.map((s) => ({
    lineStart: s.lineStart,
    lineEnd: s.lineStart + (s.text.match(/\n/g)?.length ?? 0),
    reason: s.locked,
    text: s.text,
  }));
}

async function viewOf(path: string, text: string): Promise<Body> {
  const result = await config.configSheet(path, text);
  if (!result.ok) throw new Error("no render");
  return result.value;
}

/** Locked blocks by first line. */
function byLine(blocks: readonly LineBlock[], renders: readonly (LockedRender | null)[]) {
  const out = new Map<number, LockedRender>();
  blocks.forEach((b, i) => {
    const render = renders[i];
    if (render) out.set(b.lineStart, render);
  });
  return out;
}

function markdown(blocks: Block[]): Body {
  return { kind: "markdown", blocks, outline: [], frontMatter: null };
}

const block = (lineStart: number, lineEnd: number, kind: string, html: string): Block => ({
  id: `b${lineStart}-${kind}`,
  kind,
  lineStart,
  lineEnd,
  html,
});

const line = (n: number, text: string, reason: LineBlock["reason"] = null): LineBlock => ({
  lineStart: n,
  lineEnd: n + (text.match(/\n/g)?.length ?? 0),
  reason,
  text,
});

describe("locked blocks look like View", () => {
  it("README showcase: every locked block gets the View render of its lines", async () => {
    const { text, segments } = await fixture("README.md");
    const blocks = await editorBlocks(text, segments);
    const body = await viewOf("README.md", text);
    if (body.kind !== "markdown") throw new Error("not markdown");
    const renders = lockedRenders(body, blocks);
    const at = byLine(blocks, renders);

    expect(at.get(1)).toEqual({ kind: "frontMatter", frontMatter: body.frontMatter });
    expect(at.get(73)).toEqual({ kind: "promptTag", tag: "context", edge: "open" });
    expect(at.get(78)).toEqual({ kind: "promptTag", tag: "context", edge: "close" });
    expect(at.get(80)).toEqual({ kind: "promptTag", tag: "instructions", edge: "open" });
    expect(at.get(83)).toEqual({ kind: "promptTag", tag: "instructions", edge: "close" });
    const note = at.get(85);
    expect(note?.kind).toBe("footnote");
    if (note?.kind !== "footnote") return;
    expect(note.first).toBe(true);
    const section = body.blocks.at(-1)?.html ?? "";
    const item = /<li id="fn-1">[\s\S]*?<\/li>/.exec(section)?.[0] ?? "-";
    expect(note.html).toContain(item);
    expect(note.html.startsWith(section.slice(0, section.indexOf(">") + 1))).toBe(true);

    // alerts, display math, the diagram, inline math and the paragraph using a footnote: View's own blocks, verbatim
    for (const first of [15, 26, 29, 55, 57, 63]) {
      const render = at.get(first);
      const own = blocks.find((b) => b.lineStart === first);
      expect(render?.kind, `line ${first}`).toBe("html");
      if (render?.kind !== "html" || !own) continue;
      const view = body.blocks
        .filter((b) => b.lineStart <= own.lineEnd && b.lineEnd >= own.lineStart)
        .map((b) => b.html)
        .join("\n");
      expect(render.html, `line ${first}`).toBe(view);
    }
    expect([...at.values()].filter((r) => r.kind === "source" || r.kind === "none")).toEqual([]);
    // editable blocks get nothing
    blocks.forEach((b, i) =>
      expect(renders[i] === null, `line ${b.lineStart}`).toBe(b.reason === null),
    );
  });

  it("the looks of a document's nodes, placed by the composed text, are the looks of its blocks", async () => {
    const { text, segments } = await fixture("README.md");
    const built = await buildDoc(core, text, segments);
    const body = await viewOf("README.md", text);
    const looks = lockedLooks(body, built.doc, compose(core, built, built.doc));
    const blocks = built.sources.map((s) => ({
      lineStart: s.lineStart,
      lineEnd: s.lineStart + (s.text.match(/\n/g)?.length ?? 0),
      reason: s.locked,
      text: s.text,
    }));
    const expected = lockedRenders(body, blocks);
    const nodes: (LockedRender | null)[] = [];
    built.doc.forEach((node) => nodes.push(looks.get(node) ?? null));
    expect(nodes).toEqual(expected);
    expect(looks.size).toBeGreaterThan(5);
  });

  it("CLAUDE.md: the front matter card and the prompt tag lines", async () => {
    const { text, segments } = await fixture("CLAUDE.md");
    const blocks = await editorBlocks(text, segments);
    const body = await viewOf("CLAUDE.md", text);
    const at = byLine(blocks, lockedRenders(body, blocks));
    expect(at.get(1)?.kind).toBe("frontMatter");
    expect(at.get(8)).toEqual({ kind: "promptTag", tag: "instructions", edge: "open" });
    expect(at.get(10)).toEqual({ kind: "promptTag", tag: "instructions", edge: "close" });
    expect(at.get(12)).toEqual({ kind: "promptTag", tag: "example", edge: "open" });
    expect(at.get(15)).toEqual({ kind: "promptTag", tag: "example", edge: "close" });
    expect(at.size).toBe(5);
  });

  it("link definitions, comments and dropped HTML have no View block: nothing to show", () => {
    // `<!-- a -->`, a heading, `<script>`, `[r]: http://x`, `[^unused]: ...` (never referenced: View drops it)
    const blocks = [
      line(1, "<!-- a comment -->", "html"),
      line(3, "# Title"),
      line(5, "<script>alert(1)</script>", "html"),
      line(7, "Use [r].", "reference"),
      line(9, "[r]: http://x", "definition"),
      line(11, "[^unused]: Never referenced.", "definition"),
    ];
    const body = markdown([
      block(3, 3, "h1", '<h1 data-b="h" id="title">Title</h1>'),
      block(7, 7, "p", '<p data-b="p">Use <a href="http://x">r</a>.</p>'),
    ]);
    expect(lockedRenders(body, blocks)).toEqual([
      { kind: "none" },
      null,
      { kind: "none" },
      { kind: "html", html: '<p data-b="p">Use <a href="http://x">r</a>.</p>' },
      { kind: "none" },
      { kind: "none" },
    ]);
  });

  it("a raw HTML block (View gives it the last line of the block before) still gets its render", () => {
    const div = '<div data-b="d">\n<img src="pm-asset:x.png">\n</div>';
    const blocks = [
      line(1, "Para one"),
      line(3, '<div align="center">\n<img src="x.png">\n</div>', "html"),
      line(7, "After"),
    ];
    const body = markdown([
      block(1, 1, "p", '<p data-b="p1">Para one</p>'),
      block(1, 1, "div", div),
      block(7, 7, "p", '<p data-b="p7">After</p>'),
    ]);
    expect(lockedRenders(body, blocks)[1]).toEqual({ kind: "html", html: div });
    // the same block first in the text
    const first = [
      line(1, '<div align="center">\n<img src="x.png">\n</div>', "html"),
      line(5, "After"),
    ];
    const alone = markdown([block(1, 1, "div", div), block(5, 5, "p", "<p>After</p>")]);
    expect(lockedRenders(alone, first)[0]).toEqual({ kind: "html", html: div });
  });

  it("two raw HTML blocks between the same neighbours: nobody can tell whose render is whose", () => {
    const blocks = [
      line(1, "<div>", "html"),
      line(3, "**x**"),
      line(5, "</div>", "html"),
      line(7, "<p>after</p>", "html"),
    ];
    const body = markdown([
      block(3, 3, "div", "<div>\n<p><strong>x</strong></p>\n</div>"),
      block(3, 3, "p", "<p>after</p>"),
    ]);
    const renders = lockedRenders(body, blocks);
    // the wrapper's opening line shows nothing; its View block holds an editable block
    expect(renders[0]).toEqual({ kind: "none" });
    expect(renders[2]).toEqual({ kind: "source" });
    expect(renders[3]).toEqual({ kind: "source" });
  });

  it("a View block that also covers an editable block is never used (it would show twice)", () => {
    // View fell back to one escaped `pre` (raw HTML too deep to render) while the block map did not
    const blocks = [line(1, "# Title"), line(3, "$$\nx\n$$", "math"), line(7, "tail")];
    const body = markdown([block(1, 7, "pre", "<pre><code># Title ...</code></pre>")]);
    expect(lockedRenders(body, blocks)).toEqual([null, { kind: "source" }, null]);
    // both fell back: one block, one `pre`, the same lines
    const whole = [line(1, "> > > deep\n\ntail", "parse")];
    const pre = "<pre><code>&gt; &gt; &gt; deep\n\ntail</code></pre>";
    expect(lockedRenders(markdown([block(1, 3, "pre", pre)]), whole)).toEqual([
      { kind: "html", html: pre },
    ]);
  });

  it("footnotes: matched by label, in View's order and numbers, the separator before the first", () => {
    const section =
      '<section data-b="s" class="footnotes" data-footnotes="">\n<ol>\n' +
      '<li id="fn-b">\n<p>Note B</p>\n<ul>\n<li>nested</li>\n<li>list</li>\n</ul>\n' +
      '<a href="#fnref-b" class="footnote-backref" data-footnote-backref="">↩</a>\n</li>\n' +
      '<li id="fn-a">\n<p>Note A. <a href="#fnref-a" class="footnote-backref">↩</a></p>\n</li>\n' +
      '<li id="fn-my%20note">\n<p>Spaces.</p>\n</li>\n</ol>\n</section>';
    const blocks = [
      line(1, "Text[^b] and[^a] [^my  note]", "reference"),
      line(3, "[^a]: Note A.", "footnote"),
      line(4, "[^b]: Note B\n    - nested\n    - list", "footnote"),
      line(8, "[^my  note]: Spaces.", "footnote"),
      line(10, "[^gone]: No list item.", "footnote"),
    ];
    const body = markdown([
      block(1, 1, "p", "<p>Text</p>"),
      // the section takes the lines of the first note it lists
      block(4, 6, "section", section),
    ]);
    const renders = lockedRenders(body, blocks);
    const open = '<section data-b="s" class="footnotes" data-footnotes="">';
    const itemA =
      '<li id="fn-a">\n<p>Note A. <a href="#fnref-a" class="footnote-backref">↩</a></p>\n</li>';
    expect(renders[1]).toEqual({
      kind: "footnote",
      html: `${open}\n<ol start="2">\n${itemA}\n</ol>\n</section>`,
      first: true,
    });
    const b = renders[2];
    expect(b?.kind === "footnote" && b.first).toBe(false);
    expect(b?.kind === "footnote" && b.html).toContain("<li>nested</li>\n<li>list</li>");
    expect(b?.kind === "footnote" && b.html).toContain('<ol>\n<li id="fn-b">');
    expect(renders[3]?.kind === "footnote" && renders[3].html).toContain('<ol start="3">');
    // no list item for the label: the source look stays
    expect(renders[4]).toEqual({ kind: "source" });
    expect(renders[0]).toEqual({ kind: "html", html: "<p>Text</p>" });
  });

  it("slices footnote HTML only at element boundaries (markup inside an attribute stays text)", () => {
    const hostile =
      '<section class="footnotes" data-footnotes="">\n<ol>\n' +
      '<li id="fn-1">\n<p><a href="http://a" title="<li id=&quot;fn-x&quot;><img src=x onerror=alert(1)>">a</a></p>\n</li>\n' +
      "</ol>\n</section>";
    const blocks = [line(1, "[^x]: x", "footnote"), line(2, "[^1]: a", "footnote")];
    const renders = lockedRenders(markdown([block(1, 1, "section", hostile)]), blocks);
    expect(renders[0]).toEqual({ kind: "source" });
    const one = renders[1];
    expect(one?.kind === "footnote" && one.html).toContain(
      'title="<li id=&quot;fn-x&quot;><img src=x onerror=alert(1)>">a</a></p>\n</li>\n</ol>',
    );
  });

  it("prompt blocks: paired tag lines only; a kept block inside gets its own element of the box, never the box", () => {
    const math = '<p><span data-math-style="display">x</span></p>';
    const details =
      '<details data-b="d" class="prompt-block" data-tag="context" open=""><summary>context</summary>\n' +
      `${math}\n</details>`;
    const blocks = [
      line(1, "<context>", "html"),
      line(2, "$$\nx\n$$", "math"),
      line(5, "</context>", "html"),
      line(7, "<context>", "html"),
      line(8, "never closed"),
      line(10, "<div>", "html"),
    ];
    const body = markdown([
      block(2, 4, "details", details),
      block(8, 8, "p", "<p>never closed</p>"),
    ]);
    const renders = lockedRenders(body, blocks);
    expect(renders[0]).toEqual({ kind: "promptTag", tag: "context", edge: "open" });
    expect(renders[1]).toEqual({ kind: "html", html: math });
    expect(renders[2]).toEqual({ kind: "promptTag", tag: "context", edge: "close" });
    // the box itself is drawn by the editor: its markup never comes out of here
    for (const render of renders) {
      if (render?.kind === "html") expect(render.html).not.toMatch(/<details|<summary/);
    }
    // unpaired, and a tag that is no prompt tag: plain HTML blocks
    expect(renders[3]?.kind).not.toBe("promptTag");
    expect(renders[5]?.kind).not.toBe("promptTag");
    // without the prompt blocks feature (no box in View) no line is a prompt tag
    const off = markdown([block(2, 4, "p", '<p><span data-math-style="display">x</span></p>')]);
    expect(lockedRenders(off, blocks).some((r) => r?.kind === "promptTag")).toBe(false);
  });

  it("in a box, blocks of unknown count: the others are matched from both ends; between two of them, the source", () => {
    const m1 = '<p><span data-math-style="display">a</span></p>';
    const m2 = '<p><span data-math-style="display">b</span></p>';
    const box = (inner: string[]): Block =>
      block(
        2,
        14,
        "details",
        '<details data-b="d" class="prompt-block" data-tag="context" open=""><summary>context</summary>\n' +
          `${inner.join("\n")}\n</details>`,
      );
    const blocks = [
      line(1, "<context>", "html"),
      line(2, "$$\na\n$$", "math"),
      line(6, "<div>one</div>", "html"),
      line(8, "Words."),
      line(10, "<div>two</div>", "html"),
      line(12, "$$\nb\n$$", "math"),
      line(15, "</context>", "html"),
    ];
    const both = lockedRenders(
      markdown([box([m1, "<div>one</div>", "<p>Words.</p>", "<div>two</div>", m2])]),
      blocks,
    );
    expect(both.slice(1, 6)).toEqual([
      { kind: "html", html: m1 },
      { kind: "source" },
      null,
      { kind: "source" },
      { kind: "html", html: m2 },
    ]);

    // a single block of unknown count takes what is left: a comment View drops takes nothing
    const one = [
      line(1, "<context>", "html"),
      line(2, "<!-- note -->", "html"),
      line(4, "$$\na\n$$", "math"),
      line(8, "</context>", "html"),
    ];
    expect(lockedRenders(markdown([box([m1])]), one).slice(1, 3)).toEqual([
      { kind: "none" },
      { kind: "html", html: m1 },
    ]);
    // more blocks than the box has elements: nothing in it can be told apart
    const many = [one[0], line(2, "First."), one[2], one[3]].flatMap((b) => (b ? [b] : []));
    expect(lockedRenders(markdown([box([m1])]), many)[2]).toEqual({ kind: "source" });
  });

  it("a box inside a box: its tag lines are edges too, its kept blocks get their elements in it", () => {
    const math = '<p><span data-math-style="display">x</span></p>';
    const inner =
      '<details class="prompt-block" data-tag="example" open=""><summary>example</summary>\n' +
      `<p>Input.</p>\n${math}\n</details>`;
    const outer =
      '<details data-b="d" class="prompt-block" data-tag="examples" open=""><summary>examples</summary>\n' +
      `<p>Two.</p>\n${inner}\n<hr />\n</details>`;
    const blocks = [
      line(1, "<examples>", "html"),
      line(2, "Two."),
      line(3, "<example>", "html"),
      line(4, "Input."),
      line(6, "$$\nx\n$$", "math"),
      line(9, "</example>", "html"),
      line(10, "***", "kind"),
      line(11, "</examples>", "html"),
    ];
    const renders = lockedRenders(markdown([block(2, 10, "details", outer)]), blocks);
    expect(renders).toEqual([
      { kind: "promptTag", tag: "examples", edge: "open" },
      null,
      { kind: "promptTag", tag: "example", edge: "open" },
      null,
      { kind: "html", html: math },
      { kind: "promptTag", tag: "example", edge: "close" },
      { kind: "html", html: "<hr />" },
      { kind: "promptTag", tag: "examples", edge: "close" },
    ]);
  });

  it("the front matter without the card (View: a code block) and a body that is not Markdown", () => {
    const blocks = [line(1, "---\ntitle: x\n---", "frontMatter"), line(5, "Body")];
    const pre = '<pre><code class="language-yaml">title: x\n</code></pre>';
    expect(
      lockedRenders(markdown([block(1, 3, "pre", pre), block(5, 5, "p", "<p>Body</p>")]), blocks),
    ).toEqual([{ kind: "html", html: pre }, null]);
    const text: Body = { kind: "text", text: "x", language: null };
    expect(lockedRenders(text, blocks)).toEqual([{ kind: "source" }, null]);
  });

  it("footnote | prompt box HTML not shaped as the sanitizer's (a comment, a stray close) is never cut", () => {
    const notes = (inner: string): string =>
      '<section class="footnotes" data-footnotes="">\n<ol>\n<li id="fn-1">\n<p>One</p>\n</li>\n' +
      `${inner}</ol>\n</section>`;
    for (const inner of ["<!-- -->\n", "</li>\n", '<!-- <li id="fn-1"> -->\n']) {
      const renders = lockedRenders(markdown([block(1, 1, "section", notes(inner))]), [
        line(1, "[^1]: One", "footnote"),
      ]);
      expect(renders[0], inner).toEqual({ kind: "source" });
    }
    const math = '<p><span data-math-style="display">x</span></p>';
    const box = (inner: string): string =>
      '<details class="prompt-block" data-tag="context" open=""><summary>context</summary>\n' +
      `${math}\n${inner}</details>`;
    const blocks = [
      line(1, "<context>", "html"),
      line(2, "$$\nx\n$$", "math"),
      line(5, "</context>", "html"),
    ];
    for (const inner of ["<!-- </details> -->\n", "</li>\n", "</p>\n"]) {
      const renders = lockedRenders(markdown([block(2, 4, "details", box(inner))]), blocks);
      expect(renders[1], inner).toEqual({ kind: "source" });
    }
  });

  it("a close line with attributes is no prompt tag (only a tag's name is ever shown)", () => {
    const details =
      '<details class="prompt-block" data-tag="x" open=""><summary>x</summary>\n<p>words</p>\n</details>';
    const blocks = [
      line(1, "<x>", "html"),
      line(2, "words"),
      line(3, "</x onmouseover=1>", "html"),
    ];
    const renders = lockedRenders(markdown([block(2, 2, "details", details)]), blocks);
    expect(renders.some((r) => r?.kind === "promptTag")).toBe(false);
  });
});

describe("the looks of an open document", () => {
  const TEXT = "<div>\nOne\n</div>\n\nWords.\n";
  const SEGMENTS: Segment[] = [
    { lineStart: 1, lineEnd: 3, kind: "html", lock: "html" },
    { lineStart: 5, lineEnd: 5, kind: "paragraph", lock: null },
  ];
  const render = (html: string): Body =>
    markdown([block(1, 3, "html", html), block(5, 5, "p", "<p>Words.</p>")]);

  async function opened(): Promise<{ doc: PMNode; composed: ReturnType<typeof compose> }> {
    const built = await buildDoc(core, TEXT, SEGMENTS);
    return { doc: built.doc, composed: compose(core, built, built.doc) };
  }

  it("only the answer to the latest render asked for is taken", async () => {
    const { doc, composed } = await opened();
    const book = new LookBook();
    const older = book.ask();
    const newer = book.ask();
    expect(book.take(newer, render("<div>New</div>"), doc, composed)).toBe(true);
    expect(book.take(older, render("<div>Old</div>"), doc, composed)).toBe(false);
    expect(book.get(doc.child(0))).toEqual({ kind: "html", html: "<div>New</div>" });
    expect(book.get(doc.child(1))).toBeNull();
  });

  it("a block made again with a source seen before (undo, redo) shows that look at once", async () => {
    const { doc, composed } = await opened();
    const book = new LookBook();
    book.take(book.ask(), render("<div>One</div>"), doc, composed);
    const node = doc.child(0);
    const again = node.type.create({ ...node.attrs });
    expect(again).not.toBe(node);
    expect(book.get(again)).toEqual({ kind: "html", html: "<div>One</div>" });
    expect(book.get(node.type.create({ ...node.attrs, source: "<div>\nTwo\n</div>" }))).toBeNull();
    // a later render unsure of it: the node's own look follows; what was known of the source stays
    book.take(book.ask(), markdown([block(1, 5, "div", "<div>One</div>")]), doc, composed);
    expect(book.get(node)).toEqual({ kind: "source" });
    expect(book.get(node.type.create({ ...node.attrs }))).toEqual({
      kind: "html",
      html: "<div>One</div>",
    });
  });

  it("two looks are the same when they show the same", () => {
    const front: FrontMatter = { syntax: "yaml", raw: "a: 1", fields: [] };
    const html = (text: string): LockedRender => ({ kind: "html", html: text });
    expect(sameLook(html("<p>a</p>"), html("<p>a</p>"))).toBe(true);
    expect(sameLook(html("<p>a</p>"), html("<p>b</p>"))).toBe(false);
    const note = (first: boolean): LockedRender => ({ kind: "footnote", html: "x", first });
    expect(sameLook(note(true), note(false))).toBe(false);
    const card = (): LockedRender => ({ kind: "frontMatter", frontMatter: { ...front } });
    expect(sameLook(card(), card())).toBe(true);
    expect(sameLook({ kind: "none" }, { kind: "source" })).toBe(false);
    const edge = (edge: "open" | "close"): LockedRender => ({ kind: "promptTag", tag: "a", edge });
    expect(sameLook(edge("open"), edge("close"))).toBe(false);
  });
});
