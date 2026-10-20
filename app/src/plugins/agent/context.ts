// What the window tells the agent connection about where the person is: pure helpers over the outline (the backend
// cuts, masks and checks it again before anything leaves the app).

import type { AgentSection, AgentWindowContext } from "../../api";
import type { OutlineItem } from "../../api/generated/OutlineItem";

/** The heading path and lines of the section at outline index `index` (`null` before the first heading). */
export function sectionOf(outline: readonly OutlineItem[], index: number): AgentSection | null {
  const item = outline[index];
  if (item === undefined) return null;
  const path = [item.label];
  let level = item.level;
  for (let at = index - 1; at >= 0 && level > 1; at--) {
    const above = outline[at];
    if (above !== undefined && above.level < level) {
      path.unshift(above.label);
      level = above.level;
    }
  }
  const next = outline.slice(index + 1).find((o) => o.level <= item.level);
  return {
    headingPath: path,
    lines: next !== undefined && next.line > item.line ? [item.line, next.line - 1] : null,
  };
}

/** Equal contexts are not sent twice. */
export function sameContext(a: AgentWindowContext | null, b: AgentWindowContext): boolean {
  return a !== null && JSON.stringify(a) === JSON.stringify(b);
}
