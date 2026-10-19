// Source mode: CodeMirror 6 over the edit text. Markdown (fenced code highlighted by language, loaded on demand;
// YAML front matter) or any text by file name. Completions without AI (complete.ts), a small find, keys that never
// misfire under IME, picture paste | drop, and the fence the caret is in (diagram preview).

import { autocompletion, completionKeymap } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { yamlFrontmatter } from "@codemirror/lang-yaml";
import {
  bracketMatching,
  HighlightStyle,
  LanguageDescription,
  syntaxHighlighting,
} from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { Compartment, EditorSelection, EditorState, type Extension } from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { completionSources, type CompletionHost } from "./complete";

export type SourceHost = CompletionHost & {
  changed(): void;
  selected(): void;
  save(): void;
  modeKey(source: boolean): void;
  find(): void;
  pictures(files: File[], at: number): void;
  /** The diagram fence the caret is in (`null`: none). */
  fence(fence: Fence | null): void;
};

/** A fenced block the caret is in: its language and its code. */
export type Fence = { language: string; code: string; from: number };

const style = HighlightStyle.define([
  { tag: tags.heading, fontWeight: "700", color: "var(--text)" },
  { tag: [tags.emphasis], fontStyle: "italic" },
  { tag: [tags.strong], fontWeight: "700" },
  { tag: [tags.strikethrough], textDecoration: "line-through" },
  { tag: [tags.link, tags.url], color: "var(--accent-text)" },
  { tag: [tags.monospace], color: "var(--syn-string)" },
  { tag: [tags.comment, tags.meta, tags.processingInstruction], color: "var(--syn-comment)" },
  { tag: [tags.keyword, tags.modifier, tags.operatorKeyword], color: "var(--syn-keyword)" },
  { tag: [tags.string, tags.special(tags.string), tags.regexp], color: "var(--syn-string)" },
  {
    tag: [tags.typeName, tags.className, tags.number, tags.bool, tags.atom],
    color: "var(--syn-type)",
  },
  {
    tag: [tags.propertyName, tags.attributeName, tags.definition(tags.variableName)],
    color: "var(--syn-prop)",
  },
  { tag: [tags.quote], color: "var(--muted)" },
  { tag: [tags.list, tags.contentSeparator], color: "var(--accent-text)" },
]);

const theme = EditorView.theme({
  "&": {
    color: "var(--text)",
    backgroundColor: "var(--bg)",
    fontSize: "var(--fs-code)",
  },
  ".cm-content": { fontFamily: "var(--font-code)", caretColor: "var(--text)", padding: "12px 0" },
  ".cm-scroller": { fontFamily: "var(--font-code)", lineHeight: "1.6" },
  ".cm-gutters": {
    backgroundColor: "var(--bg)",
    color: "var(--faint)",
    border: "none",
  },
  ".cm-activeLine": { backgroundColor: "var(--hover)" },
  ".cm-activeLineGutter": { backgroundColor: "var(--hover)", color: "var(--muted)" },
  "&.cm-focused": { outline: "none" },
  ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
    backgroundColor: "var(--accent-soft) !important",
  },
  ".cm-cursor": { borderLeftColor: "var(--text)" },
  ".cm-tooltip": {
    backgroundColor: "var(--bg)",
    border: "1px solid var(--border)",
    borderRadius: "var(--radius-control)",
    boxShadow: "var(--shadow-pop)",
  },
  ".cm-tooltip-autocomplete ul li[aria-selected]": {
    backgroundColor: "var(--accent-soft)",
    color: "var(--text)",
  },
  ".cm-find-match": { backgroundColor: "var(--mark)" },
});

const DIAGRAMS = new Set(["mermaid", "plantuml", "puml"]);
const FENCE = /^ {0,3}(`{3,}|~{3,})[ \t]*([^\s`]*)/;

/** The fenced block around line `at` (1-based, strictly inside the fences), when its language is a diagram. */
export function fenceAround(lines: readonly string[], at: number): Fence | null {
  const caret = at - 1;
  let open = -1;
  let marker = "";
  let language = "";
  for (let i = 0; i < lines.length; i++) {
    const match = FENCE.exec(lines[i] ?? "");
    if (open < 0) {
      if (match) {
        open = i;
        marker = match[1] ?? "";
        language = (match[2] ?? "").toLowerCase();
      }
      if (i >= caret) return null;
      continue;
    }
    const fence = match?.[1] ?? "";
    const closes =
      match !== null && !match[2] && fence[0] === marker[0] && fence.length >= marker.length;
    if (closes) {
      if (caret > open && caret < i) {
        return DIAGRAMS.has(language)
          ? { language, code: lines.slice(open + 1, i).join("\n"), from: open + 1 }
          : null;
      }
      open = -1;
      if (i >= caret) return null;
    }
  }
  if (open >= 0 && caret > open && DIAGRAMS.has(language)) {
    return { language, code: lines.slice(open + 1).join("\n"), from: open + 1 };
  }
  return null;
}

function languageFor(path: string, markdownText: boolean): Extension | Promise<Extension> {
  if (markdownText) {
    return yamlFrontmatter({
      content: markdown({ base: markdownLanguage, codeLanguages: languages }),
    });
  }
  const name = path.split("/").at(-1) ?? path;
  const found = LanguageDescription.matchFilename(languages, name);
  return found ? found.load() : [];
}

export type SourceView = {
  view: EditorView;
  text(): string;
  topLine(): number;
  scrollToLine(line: number): void;
  selection(): string;
  insert(text: string, at?: number): void;
  /** Selects the next match of `query` after the selection; false when none. */
  findNext(query: string, backwards?: boolean): boolean;
  destroy(): void;
};

export function createSource(options: {
  parent: HTMLElement;
  text: string;
  path: string;
  markdown: boolean;
  host: SourceHost;
}): SourceView {
  const { host } = options;
  const language = new Compartment();
  const loaded = languageFor(options.path, options.markdown);
  const keys = keymap.of([
    {
      key: "Mod-s",
      run: () => {
        host.save();
        return true;
      },
    },
    {
      key: "Mod-e",
      run: () => {
        host.modeKey(false);
        return true;
      },
    },
    {
      key: "Mod-Alt-e",
      run: () => {
        host.modeKey(true);
        return true;
      },
    },
    {
      key: "Mod-f",
      run: () => {
        host.find();
        return true;
      },
    },
  ]);
  const view = new EditorView({
    parent: options.parent,
    state: EditorState.create({
      doc: options.text,
      extensions: [
        keys,
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightActiveLine(),
        drawSelection(),
        history(),
        bracketMatching(),
        EditorView.lineWrapping,
        language.of(loaded instanceof Promise ? [] : loaded),
        syntaxHighlighting(style),
        theme,
        autocompletion({ override: completionSources(host), activateOnTyping: true }),
        keymap.of([...completionKeymap, ...defaultKeymap, ...historyKeymap, indentWithTab]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) host.changed();
          if (update.docChanged || update.selectionSet) {
            host.selected();
            if (!options.markdown) return;
            // a window around the caret: a long document is not split on every key
            const doc = update.state.doc;
            const line = doc.lineAt(update.state.selection.main.head).number;
            const first = Math.max(1, line - 400);
            const last = Math.min(doc.lines, line + 400);
            const lines: string[] = [];
            for (let n = first; n <= last; n++) lines.push(doc.line(n).text);
            const fence = fenceAround(lines, line - first + 1);
            host.fence(fence && { ...fence, from: fence.from + first - 1 });
          }
        }),
        EditorView.domEventHandlers({
          paste: (event, v) => {
            const files = Array.from(event.clipboardData?.files ?? []).filter((f) =>
              f.type.startsWith("image/"),
            );
            if (files.length === 0) return false;
            event.preventDefault();
            host.pictures(files, v.state.selection.main.head);
            return true;
          },
          drop: (event, v) => {
            const files = Array.from(event.dataTransfer?.files ?? []).filter((f) =>
              f.type.startsWith("image/"),
            );
            if (files.length === 0) return false;
            event.preventDefault();
            const at = v.posAtCoords({ x: event.clientX, y: event.clientY });
            host.pictures(files, at ?? v.state.selection.main.head);
            return true;
          },
        }),
      ],
    }),
  });
  if (loaded instanceof Promise) {
    void loaded.then((support) => view.dispatch({ effects: language.reconfigure(support) }));
  }
  return {
    view,
    text: () => view.state.doc.toString(),
    topLine: () => {
      const rect = view.scrollDOM.getBoundingClientRect();
      const block = view.lineBlockAtHeight(rect.top - view.documentTop + 1);
      return view.state.doc.lineAt(block.from).number;
    },
    scrollToLine: (line) => {
      const doc = view.state.doc;
      const at = doc.line(Math.min(Math.max(line, 1), doc.lines)).from;
      view.dispatch({
        selection: EditorSelection.cursor(at),
        effects: EditorView.scrollIntoView(at, { y: "start" }),
      });
    },
    selection: () => {
      const { from, to } = view.state.selection.main;
      return from === to ? "" : view.state.sliceDoc(from, to);
    },
    insert: (text, at) => {
      const pos = at ?? view.state.selection.main.head;
      view.dispatch({
        changes: { from: pos, insert: text },
        selection: EditorSelection.cursor(pos + text.length),
      });
      view.focus();
    },
    findNext: (query, backwards = false) => {
      if (query === "") return false;
      const doc = view.state.doc.toString();
      const lower = doc.toLowerCase();
      const needle = query.toLowerCase();
      const { from, to } = view.state.selection.main;
      let at = backwards ? lower.lastIndexOf(needle, from - 1) : lower.indexOf(needle, to);
      if (at < 0) at = backwards ? lower.lastIndexOf(needle) : lower.indexOf(needle);
      if (at < 0) return false;
      view.dispatch({
        selection: EditorSelection.range(at, at + needle.length),
        effects: EditorView.scrollIntoView(at, { y: "center" }),
      });
      return true;
    },
    destroy: () => view.destroy(),
  };
}
