// Source mode: CodeMirror 6 over the edit text, the raw file: nothing hidden, masked, folded or drawn over it
// (invisible characters made visible). Markdown (fenced code highlighted by language, loaded on demand; YAML front
// matter) or any text by file name. Completions without AI (complete.ts), a small find, keys that never misfire under
// IME, picture paste | drop. Find + replace (case, regexp, whole word) and go to line: `@codemirror/search`, its
// panel in the theme's colors and the app's language.

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
import {
  closeSearchPanel,
  findNext,
  findPrevious,
  gotoLine,
  openSearchPanel,
  search,
} from "@codemirror/search";
import { Compartment, EditorSelection, EditorState, type Extension } from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { configLanguage } from "./configLang";
import { completionSources, type CompletionHost } from "./complete";

export type SourceHost = CompletionHost & {
  changed(): void;
  selected(): void;
  save(): void;
  modeKey(source: boolean): void;
  pictures(files: File[], at: number): void;
};

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
  ".cm-panels": {
    backgroundColor: "var(--surface)",
    color: "var(--text)",
    fontFamily: "var(--font-ui)",
    fontSize: "var(--fs-ui-sm)",
  },
  ".cm-panels-top": { borderBottom: "1px solid var(--border)" },
  ".cm-panel.cm-search, .cm-panel.cm-gotoLine": { padding: "6px 28px 6px 12px" },
  ".cm-panel input, .cm-panel button, .cm-panel label": { fontSize: "var(--fs-ui-sm)" },
  ".cm-textfield": {
    backgroundColor: "var(--bg)",
    color: "var(--text)",
    border: "1px solid var(--border)",
    borderRadius: "var(--radius-control)",
    padding: "2px 6px",
  },
  ".cm-textfield:focus": { outline: "none", borderColor: "var(--accent)" },
  ".cm-button": {
    backgroundImage: "none",
    backgroundColor: "var(--bg)",
    color: "var(--text)",
    border: "1px solid var(--border)",
    borderRadius: "var(--radius-control)",
    padding: "2px 8px",
  },
  ".cm-button:hover": { backgroundColor: "var(--hover)" },
  ".cm-panel.cm-search [name=close]": { color: "var(--muted)" },
  ".cm-searchMatch": { backgroundColor: "var(--mark)" },
  ".cm-searchMatch-selected": { backgroundColor: "var(--mark-strong)" },
});

/** How code looks in the editors (colors from the theme tokens): the grammar's highlighting, the font, the gutter. */
export const codeLook: Extension = [syntaxHighlighting(style), theme];

/** The grammar for a file name (Markdown with its front matter and fenced code when `markdownText`); may load. */
export function languageFor(path: string, markdownText: boolean): Extension | Promise<Extension> {
  if (markdownText) {
    return yamlFrontmatter({
      content: markdown({ base: markdownLanguage, codeLanguages: languages }),
    });
  }
  const name = path.split("/").at(-1) ?? path;
  const own = configLanguage(name);
  if (own !== null && typeof own !== "string") return own;
  const found = LanguageDescription.matchFilename(languages, own ?? name);
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
  destroy(): void;
};

export function createSource(options: {
  parent: HTMLElement;
  text: string;
  path: string;
  markdown: boolean;
  host: SourceHost;
  /** The search panel's words in the app's language (CodeMirror phrase -> text). */
  phrases?: Record<string, string>;
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
    // find + replace in one panel (Ctrl+H as in common text editors), F3 | Shift+F3 next | previous, go to line
    { key: "Mod-f", run: openSearchPanel, scope: "editor search-panel" },
    { key: "Mod-h", run: openSearchPanel, scope: "editor search-panel" },
    {
      key: "F3",
      run: findNext,
      shift: findPrevious,
      scope: "editor search-panel",
      preventDefault: true,
    },
    { key: "Mod-g", run: gotoLine },
    { key: "Escape", run: closeSearchPanel, scope: "editor search-panel" },
  ]);
  const view = new EditorView({
    parent: options.parent,
    state: EditorState.create({
      doc: options.text,
      extensions: [
        keys,
        EditorState.phrases.of(options.phrases ?? {}),
        search({ top: true }),
        lineNumbers(),
        highlightSpecialChars(),
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
          if (update.docChanged || update.selectionSet) host.selected();
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
    destroy: () => view.destroy(),
  };
}
