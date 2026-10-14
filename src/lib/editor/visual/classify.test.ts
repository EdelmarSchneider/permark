// @vitest-environment jsdom
// The rules that decide which blocks Visual mode may edit (see the header of classify.ts), one by one.
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mockParseBlocks } from "../../mock/mockBlocks";
import { buildBlockMap } from "../blockModel";
import {
  blocksWithReferences,
  classifyBlock,
  hasInlineMath,
  isDiagram,
  referenceUses,
  sameMeaning,
  tidy,
} from "./classify";
import { createCore, type Core } from "./core";
import "./jsdomShims";
import type { BlockKind } from "../../types";

let core: Core;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.appendChild(host);
  core = await createCore({ host });
});
afterAll(async () => {
  await core.destroy();
});

const verdict = (text: string, kind: BlockKind = "paragraph", reference = false) =>
  classifyBlock(core, { kind, text }, reference);
const reason = (text: string, kind: BlockKind = "paragraph", reference = false): string | null => {
  const v = verdict(text, kind, reference);
  return v.editable ? null : v.reason;
};

describe("tidy", () => {
  it("reads every line ending as LF and drops the line breaks at the end, nothing else", () => {
    expect(tidy("a\r\nb\rc\n\n\n")).toBe("a\nb\nc");
    // trailing spaces can be a hard line break: kept
    expect(tidy("a  \nb  ")).toBe("a  \nb  ");
    expect(tidy("")).toBe("");
  });
});

describe("rule 1: by kind", () => {
  it.each(["frontmatter", "html", "math", "footnoteDef", "other"] as const)("locks %s", (kind) => {
    expect(reason("anything", kind)).toBe("kind");
  });

  it.each([
    "paragraph",
    "heading",
    "list",
    "table",
    "code",
    "thematicBreak",
    "blockquote",
  ] as const)("leaves %s to the other rules", (kind) => {
    const text =
      {
        heading: "# h",
        list: "- a",
        table: "| a |\n| - |\n| 1 |",
        code: "```\nx\n```",
        thematicBreak: "---",
        blockquote: "> q",
      }[kind as string] ?? "text";
    expect(reason(text, kind)).toBeNull();
  });

  it("locks a Mermaid fence, however it is written", () => {
    expect(reason("```mermaid\ngraph LR\n```", "code")).toBe("diagram");
    expect(reason("~~~ mermaid\ngraph LR\n~~~", "code")).toBe("diagram");
    expect(reason("   ```Mermaid\nx\n```", "code")).toBe("diagram");
    expect(reason("```mermaidish\nx\n```", "code")).toBeNull();
    expect(reason("```js\nmermaid\n```", "code")).toBeNull();
    expect(isDiagram("```mermaid")).toBe(true);
  });
});

describe("rule 2: inline math", () => {
  it("locks a block with `$...$` and lets dollar amounts through", () => {
    expect(reason("The cost is $x^2$ here.")).toBe("math");
    expect(reason("Both $a_b_c$ and $*x*$.")).toBe("math");
    expect(reason("It costs $5 and then $10 more.")).toBeNull();
    expect(reason("Only one $ sign.")).toBeNull();
    expect(reason("A ${placeholder} in text.")).toBeNull();
    expect(hasInlineMath("$x$")).toBe(true);
    expect(hasInlineMath("$ x $")).toBe(false);
  });
});

describe("rule 3: the editor's reading of the block", () => {
  it("locks a block that does not read as exactly one top-level node", () => {
    // a list and a paragraph the splitter took for one block (it happens with lazy continuation in some parsers)
    expect(reason("- a\n\nparagraph after")).toBe("parse");
    expect(reason("   \n  ")).toBe("parse");
  });

  it("reads the block with LF endings, whatever the file uses", () => {
    expect(verdict("line one\r\nline two", "paragraph").editable).toBe(true);
    expect(verdict("- a\r\n- b", "list").editable).toBe(true);
  });
});

describe("rule 4: references defined elsewhere", () => {
  const text =
    "See [the docs][d] and a note[^1].\n\nPlain with [a link](x.md).\n\n[d]: https://example.com\n\n[^1]: The note.\n";
  const map = buildBlockMap(text, mockParseBlocks(text));

  it("finds the blocks that use a reference or a footnote, and no other", () => {
    expect(map.blocks.map((b) => b.kind)).toEqual(["paragraph", "paragraph", "footnoteDef"]);
    expect([...blocksWithReferences(core, map)]).toEqual([0]);
  });

  it("locks them, with the reason", () => {
    expect(reason(map.blocks[0].text, "paragraph", true)).toBe("reference");
    expect(reason(map.blocks[1].text, "paragraph", false)).toBeNull();
  });

  it("locks a block that holds a definition the parser left inside it, wherever it stands", () => {
    // read on its own, the editor resolves the reference and the definition is not in its document: writing the block drops it
    expect(reason("[foo]: https://example.com\nPlain paragraph.")).toBe("definition");
    expect(reason("> Quote.\n>\n> [foo]: https://example.com", "blockquote")).toBe("definition");
    expect(reason("- [foo]: https://example.com\n- second", "list")).toBe("definition");
    expect(reason("> Quote.[^n]\n>\n> [^n]: The note.", "blockquote")).toBe("definition");
    // text that only looks like one
    expect(reason("See [the docs]: here.")).toBeNull();
    expect(reason("A line: [not a definition] and more.")).toBeNull();
  });

  it("finds a document's definitions inside a quote or a list item, and the blocks that use them", () => {
    const text =
      "> Quote.\n>\n> [foo]: https://example.com\n\n- [bar]: /b\n- two\n\nSee [x][foo] and [y][bar].\n\nPlain.\n";
    const map = buildBlockMap(text, mockParseBlocks(text));
    const uses = referenceUses(core, map);
    expect([...uses.keys()]).toEqual([2]);
    // the definitions come back as lines of Markdown, to draw the block with
    expect(uses.get(2)).toBe("[foo]: <https://example.com>\n\n[bar]: </b>");
  });

  it("keeps the footnote that a block uses with it", () => {
    const text = "Text[^1].\n\nOther.\n\n[^1]: The note,\n    continued.\n";
    const map = buildBlockMap(text, mockParseBlocks(text));
    const uses = referenceUses(core, map);
    expect([...uses.keys()]).toEqual([0]);
    expect(uses.get(0)).toBe("[^1]: The note,\n    continued.");
  });

  it("locks a block that starts with a byte order mark: the parser would drop it", () => {
    expect(reason("\uFEFF# Title")).toBe("parse");
    expect(reason("Text with \uFEFF inside")).toBeNull();
  });

  it("finds nothing in a document that cannot define one, without reading it as a whole", () => {
    const plain = "No [brackets] defined.\n\n[text][nothing]\n";
    expect(blocksWithReferences(core, buildBlockMap(plain, mockParseBlocks(plain))).size).toBe(0);
  });

  it("does not mistake a reference that nothing defines for one that is defined", () => {
    const some = "[a][missing] and [b][d]\n\n[d]: /x\n";
    const found = blocksWithReferences(core, buildBlockMap(some, mockParseBlocks(some)));
    expect([...found]).toEqual([0]);
  });
});

describe("rule 5: equivalence", () => {
  it("accepts a block the editor writes back as the same text", () => {
    for (const text of [
      "A paragraph with *emphasis*, **strong** and `code`.",
      "# A heading",
      "- one\n- two\n  - nested",
      "1. first\n2. second",
      "> quoted\n> text",
      "- [ ] open\n- [x] done",
      "```ts\nconst a = 1;\n```",
      "![alt](assets/a.png)",
    ]) {
      const v = verdict(text);
      expect(v.editable, text).toBe(true);
    }
  });

  it("accepts a block the editor writes in its own style when both mean the same", () => {
    const same = (source: string): boolean => {
      const v = verdict(source);
      if (!v.editable) return false;
      return sameMeaning(
        core,
        source,
        core.serialize(core.view.state.schema.topNodeType.create(null, v.node)),
      );
    };
    for (const source of [
      "Emphasis with _underscores_ and __strong__.",
      "* star bullet\n* second",
      "Title\n=====",
      "|a|b|\n|-|-|\n|1|2|",
      "***",
      "    indented code",
      "hard break  \nnext",
      "#NotAHeading",
    ]) {
      expect(same(source), source).toBe(true);
    }
    expect(sameMeaning(core, "a *b*", "a _b_")).toBe(true);
    expect(sameMeaning(core, "a\r\nb\r\n", "a\nb")).toBe(true);
  });

  it("locks a block the editor would change: a link title, a code fence's extra words", () => {
    expect(reason('[link](https://example.com "A title")')).toBeNull();
    // the title is kept by the editor (it is a link attribute), so this one is fine; a fence's info string after the language is not
    expect(reason("```js title=main.js\ncode\n```", "code")).toBe("differs");
  });

  it("does not take two different texts for the same", () => {
    expect(sameMeaning(core, "a *b*", "a b")).toBe(false);
    expect(sameMeaning(core, "- a\n- b", "1. a\n2. b")).toBe(false);
    expect(sameMeaning(core, "# T", "## T")).toBe(false);
    expect(sameMeaning(core, "[a](x)", "[a](y)")).toBe(false);
    expect(sameMeaning(core, "x", "x y")).toBe(false);
  });
});

describe("an editable verdict", () => {
  it("carries the node the editor made, ready to edit", () => {
    const v = verdict("Some *text*");
    expect(v.editable && v.node.type.name).toBe("paragraph");
    expect(v.editable && v.node.textContent).toBe("Some text");
  });
});
