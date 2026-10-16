// The headings of the Source editor's text, for the Outline while editing. They come from the same Markdown parser
// that colours the editor (CodeMirror's Lezer grammar), so a `#` in a code block, a quote, a list item or the front
// matter is not a heading, and only top-level headings count (the same rule as the reading view's outline).

import { markdown } from "@codemirror/lang-markdown";
import { yamlFrontmatter } from "@codemirror/lang-yaml";
import { languages } from "@codemirror/language-data";
import { LanguageDescription, ensureSyntaxTree, syntaxTree } from "@codemirror/language";
import type { EditorState, Extension } from "@codemirror/state";
import { fenceLanguage, isPlainName, resolveLanguage } from "../code/languages";
import type { HeadingInfo } from "../outline";

/**
 * The grammar for the code inside a fence, from CodeMirror's own languages (each loads the first time a fence asks for it). The
 * fence's first word is read the way the reading view reads it: `{python}`, `rust,ignore` and `js:app.js` name Python, Rust and JavaScript,
 * and so do their aliases.
 */
export function codeLanguage(info: string): LanguageDescription | null {
  const word = fenceLanguage(info);
  if (word === "" || isPlainName(word)) return null;
  const found = LanguageDescription.matchLanguageName(languages, word, false);
  if (found) return found;
  // a name only highlight.js knows by this alias ("golang", "ps1"): the language it means
  const id = resolveLanguage(word)?.id;
  return id ? LanguageDescription.matchLanguageName(languages, id, false) : null;
}

/** The language of the Source editor: Markdown, where a `---` block at the top is YAML front matter and a fence's code has its own highlighting. */
export function sourceLanguage(): Extension {
  return yamlFrontmatter({ content: markdown({ codeLanguages: codeLanguage }) });
}

/** A heading, and where its line starts in the text (the editor scrolls there). */
export type SourceHeading = HeadingInfo & { from: number };

/** How long the parse may take to reach the end of the text before the headings found so far are used. */
const PARSE_BUDGET_MS = 150;

const HEADING = /^(?:ATX|Setext)Heading([1-6])$/;

/**
 * The text a reader sees for the Markdown of a heading: links and images show their text, emphasis and code marks go, and
 * so do HTML tags and backslash escapes. Words with underscores inside (`snake_case`) are left alone.
 */
export function plainHeading(markup: string): string {
  return markup
    .replace(/<(https?:[^>\s]+)>/g, "$1")
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]*)\]\[[^\]]*\]/g, "$1")
    .replace(/<\/?[A-Za-z][^>]*>/g, "")
    .replace(/(`+)\s?(.+?)\s?\1/g, "$2")
    .replace(/(\*{1,3}|~~)(?=\S)(.+?)(?<=\S)\1/g, "$2")
    .replace(/(?<!\w)(_{1,3})(?=\S)(.+?)(?<=\S)\1(?!\w)/g, "$2")
    .replace(/\\([!-/:-@[-`{-~])/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

/** The text of an `# ATX` heading line: without the opening marks and the optional closing ones. */
function atxText(line: string): string {
  const rest = line.replace(/^\s*#{1,6}(?:[ \t]+|$)/, "");
  return /^#+[ \t]*$/.test(rest) ? "" : rest.replace(/[ \t]+#+[ \t]*$/, "");
}

/**
 * The top-level headings of the text, in order. `complete` is false when the parse did not reach the end of a very
 * long text in time: the headings found so far are returned, and a later call finds the rest.
 */
export function sourceHeadings(state: EditorState): {
  headings: SourceHeading[];
  complete: boolean;
} {
  const full = ensureSyntaxTree(state, state.doc.length, PARSE_BUDGET_MS);
  const tree = full ?? syntaxTree(state);
  const headings: SourceHeading[] = [];
  tree.iterate({
    enter(node) {
      if (node.name === "Document") return true;
      const match = HEADING.exec(node.name);
      // a heading counts when it sits directly in the Markdown document (not in a quote or a list item)
      if (match && node.node.parent?.name === "Document") {
        const line = state.doc.lineAt(node.from);
        const level = Number(match[1]);
        let raw: string;
        if (node.name.startsWith("ATX")) {
          raw = atxText(line.text);
        } else {
          // the underline is the last child; the text is every line before it
          const underline = node.node.lastChild;
          const end = underline && underline.name === "HeaderMark" ? underline.from : node.to;
          raw = state.doc
            .sliceString(node.from, end)
            .split(/\r?\n|\r/)
            .map((part) => part.trim())
            .join(" ");
        }
        headings.push({ level, text: plainHeading(raw), from: line.from });
      }
      return false;
    },
  });
  return { headings, complete: full !== null };
}
