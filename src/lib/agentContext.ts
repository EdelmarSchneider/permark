// What the window tells an agent that asks (docs/design/v0.4.md): the open document, how it is shown, the section on
// screen and the selection, in the shape the backend shares (`AgentContext`). Pure: the stores read the reading view
// or the editor, and this turns what they found into the message.

import type { AgentContext, AgentViewMode } from "./types";

/**
 * How much of a selection is read for an agent, in characters. A little more than the backend shares (4,000), so it can tell
 * that the text was cut; a selection of the whole file never crosses the boundary.
 */
export const SELECTION_READ_LIMIT = 5000;

/** A heading of the document, and the line it is on (from 1). */
export type Heading = { level: number; text: string; line: number };

/** The selected text, and its lines when the view can tell. */
export type SelectedText = { text: string; startLine?: number; endLine?: number };

/** Everything the message is made of. */
export type View = {
  document: string | null;
  mode: AgentViewMode;
  /** The headings of the document, in order, with their lines. */
  headings: readonly Heading[];
  /** The last line of the document. */
  lastLine: number;
  /** The heading of the section being read (an index into `headings`), or null above the first one. */
  current: number | null;
  selection: SelectedText | null;
  unsaved: boolean;
};

/** The headings from the top down to heading `index`: the ones it sits under, then its own. */
export function headingPath(headings: readonly Heading[], index: number): string[] {
  const own = headings[index];
  if (!own) return [];
  const path = [own.text];
  let level = own.level;
  for (let i = index - 1; i >= 0 && level > 1; i--) {
    if (headings[i].level < level) {
      path.unshift(headings[i].text);
      level = headings[i].level;
    }
  }
  return path;
}

/** The last heading that starts at or above `line`; null above the first one. */
export function headingAtLine(headings: readonly Heading[], line: number): number | null {
  let low = 0;
  let high = headings.length - 1;
  let found: number | null = null;
  while (low <= high) {
    const mid = (low + high) >> 1;
    if (headings[mid].line <= line) {
      found = mid;
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }
  return found;
}

/** The section of heading `index`, up to the next heading of any level (the unit search finds). */
export function sectionAt(
  headings: readonly Heading[],
  index: number,
  lastLine: number,
): AgentContext["section"] {
  const own = headings[index];
  if (!own) return null;
  const next = headings[index + 1];
  const end = next ? next.line - 1 : lastLine;
  return {
    headingPath: headingPath(headings, index),
    startLine: own.line,
    endLine: Math.max(own.line, end),
  };
}

/** The message for the view. Nothing is shared without a document. */
export function buildContext(view: View): AgentContext {
  if (view.document === null) {
    return { document: null, mode: null, section: null, selection: null, unsaved: false };
  }
  const section =
    view.current === null ? null : sectionAt(view.headings, view.current, view.lastLine);
  let selection: AgentContext["selection"] = null;
  if (view.selection && view.selection.text.trim() !== "") {
    const { startLine, endLine } = view.selection;
    const at = startLine === undefined ? view.current : headingAtLine(view.headings, startLine);
    selection = {
      text: view.selection.text.slice(0, SELECTION_READ_LIMIT),
      headingPath: at === null ? [] : headingPath(view.headings, at),
      ...(startLine === undefined ? {} : { startLine }),
      ...(startLine === undefined || endLine === undefined ? {} : { endLine }),
    };
  }
  return { document: view.document, mode: view.mode, section, selection, unsaved: view.unsaved };
}

/** Two messages that say the same thing. */
export function sameContext(a: AgentContext | null, b: AgentContext | null): boolean {
  return a !== null && b !== null && JSON.stringify(a) === JSON.stringify(b);
}
