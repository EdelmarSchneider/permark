// Inline syntax Source reads as View draws it. CodeMirror's GFM language reads `~x~` as subscript and `^x^` as
// superscript; View (GitHub's rules) strikes `~x~` through and has no superscript. View also marks `==text==`. The
// delimiters flank as `*` does (CommonMark), as in the renderer.

import type { DelimiterType, InlineContext, MarkdownConfig } from "@lezer/markdown";
import { Tag, tags } from "@lezer/highlight";

/** The style tag of `==marked==` text. */
export const highlightTag = Tag.define();

const SPACE = /\s|^$/;
const PUNCTUATION = /[\p{P}\p{S}]/u;

const SINGLE_TILDE: DelimiterType = { resolve: "Strikethrough", mark: "StrikethroughMark" };
const HIGHLIGHT: DelimiterType = { resolve: "Highlight", mark: "HighlightMark" };

/** A parser for a run of exactly `size` of the character `code` (a longer run is text). */
function run(code: number, size: number, type: DelimiterType) {
  return (cx: InlineContext, next: number, pos: number): number => {
    if (next !== code || cx.char(pos - 1) === code || cx.char(pos + size) === code) return -1;
    for (let i = 1; i < size; i++) if (cx.char(pos + i) !== code) return -1;
    const before = cx.slice(pos - 1, pos);
    const after = cx.slice(pos + size, pos + size + 1);
    const spaceBefore = SPACE.test(before);
    const spaceAfter = SPACE.test(after);
    const punctBefore = PUNCTUATION.test(before);
    const punctAfter = PUNCTUATION.test(after);
    return cx.addDelimiter(
      type,
      pos,
      pos + size,
      !spaceAfter && (!punctAfter || spaceBefore || punctBefore),
      !spaceBefore && (!punctBefore || spaceAfter || punctAfter),
    );
  };
}

/** Extensions of CodeMirror's GFM Markdown (`markdownLanguage`) that make it read inline syntax as View does. */
export const viewInlineSyntax: MarkdownConfig[] = [
  { remove: ["Subscript", "Superscript"] },
  {
    parseInline: [
      { name: "SingleTilde", parse: run(126, 1, SINGLE_TILDE), after: "Strikethrough" },
    ],
  },
  {
    defineNodes: [
      { name: "Highlight", style: { "Highlight/...": highlightTag } },
      { name: "HighlightMark", style: tags.processingInstruction },
    ],
    parseInline: [{ name: "Highlight", parse: run(61, 2, HIGHLIGHT), after: "Emphasis" }],
  },
];
