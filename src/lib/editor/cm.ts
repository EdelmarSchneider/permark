// The Source editor: CodeMirror 6 configured for Markdown text. This module and everything it imports is loaded
// only when someone starts editing (see SourceEditor.svelte), so reading costs none of it.

import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorSelection, EditorState, type Extension, type Text } from "@codemirror/state";
import {
  EditorView,
  drawSelection,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { SELECTION_READ_LIMIT } from "../agentContext";
import type { SourceApi } from "../bridge";
import { currentHeading } from "../outline";
import { imageMarkdown } from "./imageName";
import { sourceHeadings, sourceLanguage, type SourceHeading } from "./sourceHeadings";
import { showsAtTop, watchViewport } from "./viewport";
import { toggleMarker, wrapLink, type WrapEdit } from "./wrapSelection";

export type SourceHandlers = {
  /** The text changed; `length` is its length, so the caller can tell cheaply when it differs from the file. */
  onChange(length: number): void;
  /** The caret moved (1-based line and column). */
  onCaret(line: number, column: number): void;
  /** Images were pasted or dropped. */
  onFiles(files: File[], source: "paste" | "drop"): void;
};

export type SourceView = SourceApi & { destroy(): void };

/** Markdown colours from the design tokens, so the editor follows the light and dark themes by itself. */
const markdownStyle = HighlightStyle.define([
  { tag: tags.heading, color: "var(--text)", fontWeight: "650" },
  { tag: tags.strong, fontWeight: "700" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strikethrough, textDecoration: "line-through", color: "var(--muted)" },
  { tag: [tags.link, tags.url], color: "var(--accent)" },
  { tag: tags.monospace, color: "var(--syn-string)" },
  { tag: tags.quote, color: "var(--muted)" },
  { tag: [tags.processingInstruction, tags.contentSeparator], color: "var(--faint)" },
  { tag: tags.meta, color: "var(--faint)" },
  { tag: tags.comment, color: "var(--syn-comment)" },
  { tag: tags.keyword, color: "var(--syn-keyword)" },
  // YAML front matter
  { tag: tags.propertyName, color: "var(--syn-prop)" },
  { tag: [tags.string, tags.special(tags.string)], color: "var(--syn-string)" },
  { tag: [tags.number, tags.bool, tags.null, tags.atom], color: "var(--syn-type)" },
]);

const theme = EditorView.theme({
  "&": {
    height: "100%",
    color: "var(--text)",
    backgroundColor: "var(--bg)",
    fontSize: "var(--fs-code)",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font-code)", lineHeight: "1.65", overflow: "auto" },
  ".cm-content": { caretColor: "var(--accent)", padding: "var(--sp-4) 0 40vh" },
  ".cm-line": { padding: "0 var(--sp-4) 0 var(--sp-3)" },
  ".cm-gutters": {
    backgroundColor: "var(--bg)",
    color: "var(--faint)",
    border: "none",
    paddingLeft: "var(--sp-2)",
  },
  ".cm-activeLine": { backgroundColor: "var(--hover)" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--muted)" },
  ".cm-cursor": { borderLeftColor: "var(--accent)", borderLeftWidth: "2px" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
    backgroundColor: "color-mix(in srgb, var(--accent) 24%, transparent)",
  },
});

function applyEdit(view: EditorView, edit: WrapEdit): void {
  view.dispatch({
    changes: { from: edit.from, to: edit.to, insert: edit.insert },
    selection: EditorSelection.single(edit.selection.anchor, edit.selection.head),
    userEvent: "input.format",
  });
}

/** A command that wraps every selection through `edit` (the last selection decides where the caret goes). */
function wrapCommand(edit: (doc: string, from: number, to: number) => WrapEdit) {
  return (view: EditorView): boolean => {
    const { from, to } = view.state.selection.main;
    applyEdit(view, edit(view.state.doc.toString(), from, to));
    return true;
  };
}

/** Ctrl+B, Ctrl+I, Ctrl+K and Ctrl+Shift+X wrap the selection in Markdown (Cmd on a Mac). */
const formatKeymap = keymap.of([
  { key: "Mod-b", run: wrapCommand((d, f, t) => toggleMarker(d, f, t, "**")) },
  { key: "Mod-i", run: wrapCommand((d, f, t) => toggleMarker(d, f, t, "*")) },
  { key: "Mod-Shift-x", run: wrapCommand((d, f, t) => toggleMarker(d, f, t, "~~")) },
  { key: "Mod-`", run: wrapCommand((d, f, t) => toggleMarker(d, f, t, "`")) },
  { key: "Mod-k", run: wrapCommand(wrapLink) },
]);

function imageFiles(list: FileList | undefined | null): File[] {
  return Array.from(list ?? []);
}

export function createSourceView(
  parent: HTMLElement,
  text: string,
  line: number,
  label: string,
  handlers: SourceHandlers,
  caretAtEnd = false,
  topLine?: number,
): SourceView {
  const extensions: Extension[] = [
    lineNumbers(),
    highlightActiveLineGutter(),
    highlightActiveLine(),
    drawSelection(),
    history(),
    // a `---` block at the top is front matter (YAML), not a heading underline
    sourceLanguage(),
    syntaxHighlighting(markdownStyle),
    EditorView.lineWrapping,
    EditorState.allowMultipleSelections.of(false),
    EditorView.contentAttributes.of({ "aria-label": label, "aria-multiline": "true" }),
    formatKeymap,
    keymap.of([...defaultKeymap, ...historyKeymap]),
    theme,
    EditorView.updateListener.of((update) => {
      if (update.docChanged) handlers.onChange(update.state.doc.length);
      if (update.docChanged || update.selectionSet) {
        const head = update.state.selection.main.head;
        const at = update.state.doc.lineAt(head);
        handlers.onCaret(at.number, head - at.from + 1);
      }
    }),
    EditorView.domEventHandlers({
      paste(event) {
        const images = imageFiles(event.clipboardData?.files).filter((f) =>
          f.type.startsWith("image/"),
        );
        if (images.length === 0) return false;
        event.preventDefault();
        handlers.onFiles(images, "paste");
        return true;
      },
      drop(event, view) {
        const files = imageFiles(event.dataTransfer?.files);
        if (files.length === 0) return false;
        event.preventDefault();
        const at = view.posAtCoords({ x: event.clientX, y: event.clientY });
        if (at !== null) view.dispatch({ selection: EditorSelection.cursor(at) });
        handlers.onFiles(files, "drop");
        return true;
      },
    }),
  ];

  const state = EditorState.create({ doc: text, extensions });
  const view = new EditorView({ state, parent });
  const lineAt = (n: number): { from: number; to: number } =>
    view.state.doc.line(Math.min(Math.max(n + 1, 1), view.state.doc.lines));
  const lineStart = (n: number): number => lineAt(n).from;
  // `topLine` is the line that goes to the top of the view (the place the other mode was showing); without it the caret is only made visible
  view.dispatch({
    selection: EditorSelection.cursor(caretAtEnd ? lineAt(line).to : lineStart(line)),
    scrollIntoView: topLine === undefined,
  });
  if (topLine !== undefined && topLine > 0) {
    view.dispatch({
      effects: EditorView.scrollIntoView(lineStart(topLine), { y: "start", yMargin: 8 }),
    });
  }

  const insertAtSelection = (insert: string): void => {
    const { from, to } = view.state.selection.main;
    view.dispatch({
      changes: { from, to, insert },
      selection: EditorSelection.cursor(from + insert.length),
      userEvent: "input.paste",
      scrollIntoView: true,
    });
  };

  // the headings of the text, for the Outline: read again only when the text is not the one they were read from
  let headingsOf: { doc: Text; list: SourceHeading[] } | null = null;
  const headingList = (): SourceHeading[] => {
    const { doc } = view.state;
    if (headingsOf?.doc === doc) return headingsOf.list;
    const { headings, complete } = sourceHeadings(view.state);
    headingsOf = complete ? { doc, list: headings } : null;
    return headings;
  };

  return {
    mode: "source",
    headings: () => headingList().map(({ level, text }) => ({ level, text })),
    scrollToHeading(index) {
      const heading = headingList()[index];
      if (!heading) return;
      view.dispatch({
        effects: EditorView.scrollIntoView(heading.from, { y: "start", yMargin: 8 }),
      });
    },
    currentHeading() {
      const list = headingList();
      if (list.length === 0) return null;
      const scroller = view.scrollDOM;
      const height = scroller.clientHeight;
      const atBottom = scroller.scrollTop + height >= scroller.scrollHeight - 2;
      // where the document starts on screen, measured once; a heading's top is that plus its offset in the document
      const base = view.documentTop - scroller.getBoundingClientRect().top;
      return currentHeading(list.length, (i) => base + view.lineBlockAt(list[i].from).top, {
        line: height / 3,
        bottomEdge: atBottom ? height : undefined,
      });
    },
    onViewport: (handler) => watchViewport(view.scrollDOM, handler),
    getText: () => view.state.doc.toString(),
    setText(next, atLine) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: next },
        selection: EditorSelection.cursor(0),
      });
      view.dispatch({
        selection: EditorSelection.cursor(lineStart(atLine)),
        scrollIntoView: true,
      });
    },
    caretLine: () => view.state.doc.lineAt(view.state.selection.main.head).number - 1,
    selection() {
      const { from, to } = view.state.selection.main;
      if (from === to) return null;
      const { doc } = view.state;
      return {
        // only what an agent can be told is read, so a selection of the whole file stays cheap
        text: doc.sliceString(from, Math.min(to, from + SELECTION_READ_LIMIT)),
        startLine: doc.lineAt(from).number,
        endLine: doc.lineAt(to).number,
      };
    },
    topLine() {
      // the document's own coordinates of the top edge of the view
      const edge = view.scrollDOM.getBoundingClientRect().top;
      let block = view.lineBlockAtHeight(Math.max(0, edge - view.documentTop));
      // a line cut off at the edge is the one that was scrolled past
      const top = view.documentTop + block.top;
      if (block.height > 0 && !showsAtTop(top, top + block.height, edge)) {
        block = view.lineBlockAtHeight(block.bottom + 1);
      }
      return view.state.doc.lineAt(block.from).number - 1;
    },
    focus: () => view.focus(),
    insertImage: (rel) => insertAtSelection(imageMarkdown(rel)),
    applyChanges(edits) {
      // positions are relative to the document as it is now; CodeMirror maps the caret through the changes
      view.dispatch({
        changes: edits.map(({ from, to, insert }) => ({ from, to, insert })),
        userEvent: "input.link",
      });
    },
    destroy: () => view.destroy(),
  };
}
