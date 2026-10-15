// The headings of a Markdown text with the line each is on, read with the parser of the Source editor, so the
// list is the same one the Outline shows while editing. The agent connection needs it for both editors (Visual mode
// has the text a save would write, not a line for each heading). Loaded on first use, like the editors.

import { EditorState } from "@codemirror/state";
import type { Heading } from "../agentContext";
import { sourceHeadings, sourceLanguage } from "./sourceHeadings";

export function headingLinesOf(text: string): { headings: Heading[]; lastLine: number } {
  const state = EditorState.create({ doc: text, extensions: [sourceLanguage()] });
  const { headings } = sourceHeadings(state);
  return {
    headings: headings.map((heading) => ({
      level: heading.level,
      text: heading.text,
      line: state.doc.lineAt(heading.from).number,
    })),
    lastLine: state.doc.lines,
  };
}
