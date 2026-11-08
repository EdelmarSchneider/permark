// Token colors for Edit mode's code blocks: View's highlighting of the same text (`highlightFence`, so the same
// language, guess and size limit), its HTML read back as runs of text with the classes around each. The HTML is parsed
// in an inert template and only read: it never enters the editor. The token styles load with this module.

import type { CodeSpan } from "../../core/plugins/props";
import { highlightFence } from "./highlighter";
import "./tokens.css";

/** The text runs of highlight.js's HTML for `source`, with the classes around each; null: its text is not `source`. */
function spansOf(html: string, source: string): CodeSpan[] | null {
  const template = document.createElement("template");
  template.innerHTML = html;
  const spans: CodeSpan[] = [];
  let at = 0;
  const walk = (parent: Node, classes: readonly string[]): void => {
    for (const child of parent.childNodes) {
      if (child instanceof Text) {
        const to = at + child.data.length;
        if (classes.length > 0 && to > at) spans.push({ from: at, to, classes });
        at = to;
      } else if (child instanceof Element) {
        const className = child.getAttribute("class");
        walk(child, className ? [...classes, className] : classes);
      }
    }
  };
  walk(template.content, []);
  return template.content.textContent === source ? spans : null;
}

/** The runs View colors in a fenced block's text; null: View draws it plain. */
export async function codeSpans(source: string, info: string): Promise<CodeSpan[] | null> {
  const { html } = await highlightFence(source, info);
  return html === null ? null : spansOf(html, source);
}
