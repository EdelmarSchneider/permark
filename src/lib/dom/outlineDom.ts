// Reads the rendered document for the outline: the top-level blocks in order (headings with their level and
// text, a flag for the blocks that carry a live-change mark) and the heading elements themselves.

import type { BlockInfo } from "../outline";
import { headingText } from "./domUtil";

const HEADING = /^H([1-6])$/;

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
