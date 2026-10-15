// Reads the rendered document for the outline: the top-level blocks in order (headings with their level and
// text, a flag for the blocks that carry a live-change mark) and the heading elements themselves.

import type { Heading } from "../agentContext";
import type { BlockInfo } from "../outline";
import { parseSourcepos } from "../sourcepos";
import { headingText } from "./domUtil";

const HEADING = /^H([1-6])$/;

/** The headings of the rendered document with the line each is on, and the last line of the document. */
export function readHeadingLines(body: HTMLElement): { headings: Heading[]; lastLine: number } {
  const headings: Heading[] = [];
  let line = 1;
  let lastLine = 1;
  for (const el of Array.from(body.children)) {
    const range = parseSourcepos(el.getAttribute("data-sourcepos"));
    if (range) {
      line = range.startLine;
      lastLine = Math.max(lastLine, range.endLine);
    }
    const level = HEADING.exec(el.tagName);
    // a heading without a position takes the line of the block before it, so the list keeps the outline's order
    if (level) headings.push({ level: Number(level[1]), text: headingText(el), line });
  }
  return { headings, lastLine };
}

export function readBlocks(body: HTMLElement): { blocks: BlockInfo[]; headings: HTMLElement[] } {
  const blocks: BlockInfo[] = [];
  const headings: HTMLElement[] = [];
  for (const el of Array.from(body.children)) {
    // a changed block, or the marker that stands where blocks were removed
    const changed = el.classList.contains("tm-changed") || el.classList.contains("tm-removed");
    const level = HEADING.exec(el.tagName);
    if (level && el instanceof HTMLElement) {
      blocks.push({ level: Number(level[1]), text: headingText(el), changed });
      headings.push(el);
    } else {
      blocks.push({ level: null, text: "", changed });
    }
  }
  return { blocks, headings };
}
