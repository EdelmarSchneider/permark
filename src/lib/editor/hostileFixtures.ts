// Markdown that is hard for an editor to round-trip, for the corpus test. The
// strings are written with explicit escapes so the bytes in the test are the bytes the author meant.

import { BOM } from "./blockModel";

export const HOSTILE: Record<string, string> = {
  "nested lists with mixed markers and tabs":
    "- one\n  - two\n    - three\n\t- tabbed\n* star\n+ plus\n\n1. first\n   1. nested\n2. second\n\n10) paren list\n",
  "tables with alignment and pipes":
    "| a | b | c |\n|:--|:-:|--:|\n| `x | y` | \\| escaped | 3 |\n| long cell with **bold** | | |\n\nTable without outer pipes:\n\na | b\n--|--\n1 | 2\n",
  "html blocks":
    '<div class="note">\n<p>raw</p>\n</div>\n\n<!-- a comment\n\nthat spans a blank line -->\n\n<details>\n<summary>More</summary>\n\nHidden **text**.\n\n</details>\n\n<br>\n',
  "footnotes and reference links":
    'Text with a note[^1] and a [reference link][ref] and [another][].\n\n[^1]: The note,\n    continued here.\n\n[ref]: https://example.com/a "Title"\n[another]: <https://example.com/b>\n\nLast paragraph.\n',
  "setext headings and thematic breaks":
    "Title\n=====\n\nSubtitle\n--------\n\n***\n\n___\n\n- - -\n\nText\n",
  "lazy continuation":
    "> a quote\ncontinued lazily\n\n- item\ncontinued lazily\n\n1. numbered\ncontinued\n",
  "trailing spaces and hard breaks":
    "line with two spaces  \nnext line\\\nand a backslash break\n\ntrailing tab\t\n\n   indented three spaces\n",
  "indented code and fences":
    "    indented code\n    more\n\n```js\nconst a = 1;\n\n\n// blank lines above\n```\n\n~~~~\ntilde fence ```\n~~~~\n\n````md\n```inner\n```\n````\n",
  "front matter and an hr":
    '---\ntitle: A "quoted" title\ntags: [a, b]\nnested:\n  key: value\n---\n\n# Heading\n\n---\n\nAfter a rule.\n',
  "an hr first is not front matter": "---\n\nJust a rule above this text.\n",
  "unclosed front matter": "---\ntitle: never closed\n\n# Heading\n",
  "math and mermaid":
    "Inline $x^2$ math.\n\n$$\n\\int_0^1 x\\,dx\n$$\n\n$$a + b$$\n\n```mermaid\ngraph LR\n  A --> B\n```\n\n```math\ny = mx + b\n```\n",
  "alerts and task lists":
    "> [!NOTE]\n> A note.\n\n> [!WARNING]\n> Careful.\n\n- [x] done\n- [ ] todo\n  - [ ] nested\n",
  "emoji, CJK, accents and right to left":
    "# 日本語の見出し 😀\n\nCafé crème — naïve façade, Zürich, Ångström, ñandú.\n\nمرحبا بالعالم 🌍\n\né (combining accent) and \u{1F468}‍\u{1F469}‍\u{1F467} (joined emoji)\n",
  "no final newline": "# Title\n\nThe last line has no newline",
  "many final newlines": "# Title\n\nBody\n\n\n\n",
  "leading blank lines": "\n\n\n# Title after blank lines\n",
  "whitespace only": "  \n\t\n   \n",
  "an empty file": "",
  "a single character": "x",
  "crlf throughout":
    "# Title\r\n\r\nFirst.\r\nSecond line.\r\n\r\n- a\r\n- b\r\n\r\n```\r\ncode\r\n```\r\n",
  "crlf with a bom": `${BOM}---\r\ntitle: t\r\n---\r\n\r\n# Title\r\n\r\nBody.\r\n`,
  "a bom with lf": `${BOM}# Title\n\nBody.\n`,
  "a bom alone": BOM,
  "lone cr line endings": "# Title\r\rBody line\rsecond\r",
  "mixed line endings": "# Title\r\n\r\nUnix line.\nWindows line.\r\n\n- a\r\n- b\n",
  "very long lines": `# ${"long ".repeat(4000)}\n\n${"word ".repeat(20000)}\n`,
  "many tiny blocks": "a\n\n".repeat(3000),
  "blocks separated by lots of blank lines": "a\n\n\n\n\n\nb\n\n\n\nc\n",
  "link definitions between paragraphs": "one\n\n[x]: /a\n\ntwo\n\n[y]: /b 'T'\n[z]: /c\n\nthree\n",
  "a document of only a definition": "[x]: https://example.com\n",
  "unbalanced fence at the end": "text\n\n```rust\nfn main() {}\n",
  "html comment only": "<!-- nothing else -->\n",
  "headings without a space": "#NotAHeading\n\n####### seven hashes\n\n# Real\n",
  // a definition the parser leaves inside the block it touches, and a reference to it further down
  "definition directly before a paragraph":
    "[foo]: https://example.com\nPlain paragraph.\n\nSee [x][foo] here.\n",
  "definition inside a blockquote":
    "> Quote.\n>\n> [foo]: https://example.com\n\nSee [x][foo] here.\n",
  "definition inside a list item": "- [foo]: https://example.com\n- second\n\nSee [x][foo] here.\n",
  "definition after a list item, indented":
    "- item\n\n  [foo]: https://example.com\n\nSee [x][foo] here.\n",
  "footnote defined inside a blockquote":
    "> Quote.\n>\n> [^n]: The note.\n\nSee the note[^n] here.\n",
  // blocks that only stay apart because of what they are: one newline, no blank line
  "a paragraph, one newline, a heading": "Intro text\n# Heading\n\nAfter.\n",
  "a paragraph, one newline, a fence": "Para text\n```\ncode\n```\n\nAfter.\n",
  "a paragraph, one newline, a list": "Intro:\n- a\n- b\n\nOutro.\n",
  "a list, one newline, a heading": "- a\n- b\n# H\n",
  "a paragraph, one newline, a quote": "Lead\n> quoted\n\nAfter.\n",
  // lists kept apart only by their marker
  "adjacent lists with different bullets": "- a\n- b\n\n* c\n* d\n",
  "adjacent lists with different number delimiters": "1. a\n2. b\n1) c\n2) d\n",
  "a paragraph after a list": "Para\n\n- a\n- b\n\nNext\n",
  // text the editor reads differently from how it was written
  "a link at the end and at the start of a paragraph":
    "See [docs](a.md)\n\n<https://x.com> autolink\n\nBefore [b](b.md) after.\n",
  "character references and bare urls":
    "a&nbsp;b and &copy; sign\n\nVisit https://example.com/page now, ~x~ struck.\n",
};
