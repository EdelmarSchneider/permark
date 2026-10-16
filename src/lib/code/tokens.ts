// highlight.js writes its result as HTML (`<span class="hljs-keyword">fn</span> main`). The reading view puts that HTML in the page; Visual
// mode cannot (the editor owns the text), so it needs the same result as ranges of the text to draw as decorations. This turns one into
// the other, keeping what the style sheet does with it: a stretch of text is coloured by the innermost scope around it that
// `markdown.css` has a rule for, so both modes show the same colours.

/** The `hljs-*` scopes that `src/styles/markdown.css` gives a colour or a font style (a test keeps the two lists equal). */
export const STYLED_SCOPES: ReadonlySet<string> = new Set([
  "hljs-comment",
  "hljs-quote",
  "hljs-keyword",
  "hljs-selector-tag",
  "hljs-literal",
  "hljs-doctag",
  "hljs-section",
  "hljs-string",
  "hljs-regexp",
  "hljs-addition",
  "hljs-type",
  "hljs-built_in",
  "hljs-meta",
  "hljs-attr",
  "hljs-attribute",
  "hljs-property",
  "hljs-variable",
  "hljs-number",
  "hljs-symbol",
  "hljs-name",
  "hljs-title",
  "hljs-params",
  "hljs-template-variable",
  "hljs-selector-class",
  "hljs-selector-id",
  "hljs-deletion",
  "hljs-emphasis",
  "hljs-strong",
]);

/** A stretch of the code's text and the class it is drawn with. Offsets are in UTF-16 units of the text. */
export type TokenRange = { from: number; to: number; className: string };

const ENTITIES: Readonly<Record<string, string>> = {
  "&amp;": "&",
  "&lt;": "<",
  "&gt;": ">",
  "&quot;": '"',
  "&#x27;": "'",
  "&#39;": "'",
};

const PIECE = /<span class="([^"]*)">|<\/span>|&(?:amp|lt|gt|quot|#x27|#39);|[^<&]+|[<&]/g;

/**
 * The ranges of the text that highlight.js's HTML colours. Text outside any styled scope has no range. Neighbouring stretches with
 * the same class are one range.
 */
export function tokenRanges(html: string): TokenRange[] {
  const ranges: TokenRange[] = [];
  const stack: string[] = [];
  let offset = 0;
  const innermost = (): string | null => {
    for (let i = stack.length - 1; i >= 0; i--) {
      if (stack[i].split(" ").some((scope) => STYLED_SCOPES.has(scope))) return stack[i];
    }
    return null;
  };
  for (const match of html.matchAll(PIECE)) {
    const piece = match[0];
    if (match[1] !== undefined) {
      stack.push(match[1]);
      continue;
    }
    if (piece === "</span>") {
      stack.pop();
      continue;
    }
    const text = ENTITIES[piece] ?? piece;
    const className = innermost();
    if (className !== null) {
      const last = ranges[ranges.length - 1];
      if (last && last.to === offset && last.className === className) last.to += text.length;
      else ranges.push({ from: offset, to: offset + text.length, className });
    }
    offset += text.length;
  }
  return ranges;
}
