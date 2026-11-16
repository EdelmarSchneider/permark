import { describe, expect, it } from "vitest";
import { blockGolden, slugOf, sortedAttributes } from "./digest.ts";

describe("blockGolden", () => {
  it("lists front matter, outline and blocks without their ids", () => {
    const golden = blockGolden({
      path: "corpus/a.md",
      format: "markdown",
      body: {
        kind: "markdown",
        frontMatter: { syntax: "yaml", fields: [] },
        outline: [
          { level: 1, label: "Title", line: 1 },
          { level: 2, label: "Part", line: 5 },
        ],
        blocks: [
          {
            kind: "h1",
            lineStart: 1,
            lineEnd: 1,
            html: '<h1 data-b="72ae5c74" id="title">Title</h1>',
          },
          { kind: "p", lineStart: 3, lineEnd: 4, html: '<p data-b="6fc5">a\nb</p>' },
          { kind: "hr", lineStart: 6, lineEnd: 6, html: null },
        ],
      },
    });
    expect(golden).toBe(
      [
        "# corpus/a.md",
        "format: markdown",
        'front matter: {"syntax":"yaml","fields":[]}',
        "outline: Title (line 1)",
        "outline:   Part (line 5)",
        "--- h1 1-1",
        '<h1 id="title">Title</h1>',
        "--- p 3-4",
        "<p>a\nb</p>",
        "--- hr 6-6",
        "",
        "",
      ].join("\n"),
    );
  });
});

describe("sortedAttributes", () => {
  it("orders each start tag's attributes, values and text untouched", () => {
    expect(
      sortedAttributes(
        '<p><audio src="a b.wav" preload="metadata" controls=""></audio> x="1" <br/></p>',
      ),
    ).toBe('<p><audio controls="" preload="metadata" src="a b.wav"></audio> x="1" <br/></p>');
    expect(sortedAttributes('<a title="1 > 0" href="#x">t</a>')).toBe(
      '<a href="#x" title="1 > 0">t</a>',
    );
    expect(sortedAttributes('<input type="checkbox" disabled checked>')).toBe(
      '<input checked disabled type="checkbox">',
    );
  });
});

describe("slugOf", () => {
  it("makes one file name per page", () => {
    expect(slugOf("demo/README.md")).toBe("demo__README");
    expect(slugOf("corpus/security/xss-attacks.md")).toBe("corpus__security__xss-attacks");
    expect(slugOf("demo/a b#c.markdown")).toBe("demo__a-b-c");
  });
});
