import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { plainHeading, sourceHeadings, sourceLanguage } from "./sourceHeadings";

const state = (doc: string) => EditorState.create({ doc, extensions: [sourceLanguage()] });
/** `[level, text]` of every top-level heading. */
const heads = (doc: string) => sourceHeadings(state(doc)).headings.map((h) => [h.level, h.text]);

describe("sourceHeadings", () => {
  it("reads ATX headings of every level, in order", () => {
    const doc = "# One\n\n## Two\n\n### Three\n\n#### Four\n\n##### Five\n\n###### Six\n";
    expect(heads(doc)).toEqual([
      [1, "One"],
      [2, "Two"],
      [3, "Three"],
      [4, "Four"],
      [5, "Five"],
      [6, "Six"],
    ]);
  });

  it("drops the closing hashes and the spaces around the text", () => {
    expect(heads("## Closed ##\n\n#   Spaced   \n\n### Both ###   \n")).toEqual([
      [2, "Closed"],
      [1, "Spaced"],
      [3, "Both"],
    ]);
  });

  it("keeps an empty heading, and a hash that is part of a word is not one", () => {
    expect(heads("#\n\n#hashtag\n\n## ##\n")).toEqual([
      [1, ""],
      [2, ""],
    ]);
  });

  it("reads underlined headings, including a heading over several lines", () => {
    expect(heads("Title\n=====\n\nSub\n---\n\nTwo lines\nof text\n===\n")).toEqual([
      [1, "Title"],
      [2, "Sub"],
      [1, "Two lines of text"],
    ]);
  });

  it("ignores what is not a top-level heading", () => {
    const doc = [
      "---",
      "title: x",
      "# a comment in the front matter",
      "---",
      "# Real",
      "",
      "> ## Quoted",
      "",
      "- item",
      "",
      "  ### Inside the list",
      "",
      "```md",
      "# In a code block",
      "```",
      "",
      "<!--",
      "# In a comment",
      "-->",
      "",
      "## Also real",
      "",
    ].join("\n");
    expect(heads(doc)).toEqual([
      [1, "Real"],
      [2, "Also real"],
    ]);
  });

  it("gives the start of the heading's line, so the editor can scroll to it", () => {
    const doc = "intro\n\n## Second\n\nbody\n\nThird\n-----\n";
    const found = sourceHeadings(state(doc)).headings;
    expect(found.map((h) => h.from)).toEqual([doc.indexOf("## Second"), doc.indexOf("Third")]);
  });

  it("has nothing to say about an empty text or one without headings", () => {
    expect(heads("")).toEqual([]);
    expect(heads("just a paragraph\n\n- and a list\n")).toEqual([]);
  });

  it("reports a complete parse for a small text", () => {
    expect(sourceHeadings(state("# A\n")).complete).toBe(true);
  });

  it("handles a long document with many headings", () => {
    const doc = Array.from({ length: 1500 }, (_, i) => `## Section ${i + 1}\n\ntext\n`).join("\n");
    // Each call parses for a bounded time and picks up where the last one stopped, so a slow machine needs a few.
    const s = state(doc);
    let result = sourceHeadings(s);
    for (let tries = 0; !result.complete && tries < 1000; tries++) result = sourceHeadings(s);
    expect(result.complete).toBe(true);
    const found = result.headings;
    expect(found).toHaveLength(1500);
    expect(found[1499].text).toBe("Section 1500");
  });
});

describe("plainHeading", () => {
  it("shows the text a reader sees, not the Markdown", () => {
    expect(plainHeading("The **bold** and *italic* and ~~gone~~ words")).toBe(
      "The bold and italic and gone words",
    );
    expect(plainHeading("Use `cargo test` here")).toBe("Use cargo test here");
    expect(plainHeading("See [the guide](guide.md) and ![logo](a.png)")).toBe(
      "See the guide and logo",
    );
    expect(plainHeading("A [ref link][ref] and <b>tag</b>")).toBe("A ref link and tag");
    expect(plainHeading("Costs \\$5 \\# not a mark")).toBe("Costs $5 # not a mark");
    expect(plainHeading("An <https://example.com> link")).toBe("An https://example.com link");
  });

  it("leaves underscores and stars inside words alone", () => {
    expect(plainHeading("snake_case_name and a_b_c")).toBe("snake_case_name and a_b_c");
    expect(plainHeading("_emphasis_ here")).toBe("emphasis here");
  });

  it("collapses runs of white space", () => {
    expect(plainHeading("  two   words \t here ")).toBe("two words here");
  });
});
