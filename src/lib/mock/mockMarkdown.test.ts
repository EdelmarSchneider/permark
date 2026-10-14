import { describe, expect, it } from "vitest";
import { mockParseBlocks } from "./mockBlocks";
import { blockToMarkdown, htmlToMarkdown, markdownToView } from "./mockMarkdown";

describe("mockParseBlocks", () => {
  const kinds = (text: string): string[] => mockParseBlocks(text).blocks.map((b) => b.kind);

  it("finds the kinds the backend reports", () => {
    const text =
      "---\ntitle: x\n---\n\n# H\n\npara\n\n- a\n- b\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```ts\ncode\n\nmore\n```\n\n<div>\nhtml\n</div>\n\n$$\nx\n$$\n\n[^1]: note\n\n---\n\n> quote\n\n> [!NOTE]\n> alert\n\nSetext\n===\n";
    expect(kinds(text)).toEqual([
      "frontmatter",
      "heading",
      "paragraph",
      "list",
      "table",
      "code",
      "html",
      "math",
      "footnoteDef",
      "thematicBreak",
      "blockquote",
      "other",
      "heading",
    ]);
  });

  it("counts bytes, not characters, and leaves link definitions in the separators", () => {
    const text = "# é😀\n\n[a]: /x\n\nend\n";
    const { blocks } = mockParseBlocks(text);
    expect(blocks.map((b) => [b.startByte, b.endByte])).toEqual([
      [0, 8],
      [19, 22],
    ]);
  });

  it("reports the line ending", () => {
    expect(mockParseBlocks("a\r\nb\r\n").eol).toBe("crlf");
    expect(mockParseBlocks("a\nb\r\n").eol).toBe("mixed");
    expect(mockParseBlocks("a\nb\n").eol).toBe("lf");
  });
});

describe("the mock Markdown converters", () => {
  it("turn the mock HTML blocks into Markdown", () => {
    const h = '<h2><a href="#x" aria-hidden="true" class="anchor" id="x"></a>Event model</h2>';
    expect(blockToMarkdown(h)).toBe("## Event model");
    expect(blockToMarkdown("<p>A <em>b</em> and <strong>c</strong> <code>d</code>.</p>")).toBe(
      "A *b* and **c** `d`.",
    );
    expect(
      blockToMarkdown(
        '<ul><li>one</li><li><input type="checkbox" checked="" disabled="" /> done</li></ul>',
      ),
    ).toBe("- one\n- [x] done");
    expect(blockToMarkdown('<pre><code class="language-ts">a &lt; b\n</code></pre>')).toBe(
      "```ts\na < b\n```",
    );
    expect(
      blockToMarkdown(
        "<table><thead><tr><th>A</th><th>B</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table>",
      ),
    ).toBe("| A | B |\n| --- | --- |\n| 1 | 2 |");
    expect(htmlToMarkdown(["<p>x</p>"], [["status", "Draft"]])).toBe(
      "---\nstatus: Draft\n---\n\nx\n",
    );
  });

  it("render the Markdown text back as a view with source positions", () => {
    const text =
      "---\nstatus: Draft\n---\n\n# Title\n\nA **bold** [link](a.md) and `code`.\n\n- one\n- [x] two\n";
    const view = markdownToView(text);
    expect(view.title).toBe("Title");
    expect(view.frontmatter).toEqual([["status", "Draft"]]);
    expect(view.blocks).toHaveLength(3);
    expect(view.html).toContain('<h1 data-sourcepos="5:1-5:1">');
    expect(view.html).toContain("<strong>bold</strong>");
    expect(view.html).toContain('<a href="a.md">link</a>');
    expect(view.html).toContain("<code>code</code>");
    expect(view.html).toContain('<input type="checkbox" checked="" disabled="" />');
  });

  it("round-trip a mock document through its text", () => {
    const html = ['<h1><a href="#t" class="anchor" id="t"></a>T</h1>', "<p>Plain text.</p>"];
    const view = markdownToView(htmlToMarkdown(html));
    expect(view.title).toBe("T");
    expect(view.html).toContain("Plain text.");
  });
});
