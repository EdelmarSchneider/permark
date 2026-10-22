// Highlighted code as one element per line, for a viewer that wraps long lines and numbers each one: a token that
// goes over a line break (a comment, a string) is closed at the end of the line and opened again on the next, so
// every line stands alone. Pure.

/** Tags and line breaks of escaped HTML (text never holds `<`). */
const TOKENS = /<\/?([a-zA-Z][a-zA-Z0-9-]*)[^>]*>|\n/g;

/** The lines of escaped HTML (the empty piece after a final line break is no line; a carriage return goes). */
export function htmlLines(source: string): string[] {
  // a carriage return before a line break (tags may close in between) goes
  const html = source.replace(/\r((?:<[^>]*>)*)\n/g, "$1\n");
  const out: string[] = [];
  const open: { tag: string; name: string }[] = [];
  let line = "";
  let at = 0;
  /** Text came after the last line break (tags alone make no line). */
  let text = false;
  let breaks = 0;
  const end = (): string => {
    let closed = line.endsWith("\r") ? line.slice(0, -1) : line;
    for (let i = open.length - 1; i >= 0; i--) closed += `</${open[i]?.name ?? "span"}>`;
    return closed;
  };
  for (const match of html.matchAll(TOKENS)) {
    const before = html.slice(at, match.index);
    if (before !== "") text = true;
    line += before;
    at = match.index + match[0].length;
    const token = match[0];
    if (token === "\n") {
      out.push(end());
      breaks += 1;
      text = false;
      line = open.map((o) => o.tag).join("");
    } else if (token.startsWith("</")) {
      open.pop();
      line += token;
    } else {
      line += token;
      if (!token.endsWith("/>")) open.push({ tag: token, name: match[1] ?? "span" });
    }
  }
  const rest = html.slice(at);
  // text after the last line break (or no line break at all): one more line
  if (rest !== "" || text || breaks === 0) {
    line += rest;
    out.push(end());
  }
  return out;
}

/** The most indentation the further rows of a wrapped line keep (as the text viewer's `MAX_LEAD`). */
const MAX_LEAD = 24;

/** The columns of a line's leading spaces | tabs (tab stops of 8), at most `MAX_LEAD`. */
export function leadCols(line: string): number {
  let width = 0;
  for (const char of line) {
    if (char === " ") width += 1;
    else if (char === "\t") width += 8 - (width % 8);
    else break;
    if (width >= MAX_LEAD) return MAX_LEAD;
  }
  return width;
}

/**
 * Each line in a `span.ln` (its line break inside it, so a copy keeps one per line and an empty line its height);
 * `leads[i]` = the indentation line i's further rows keep (`--lead`).
 */
export function lineSpans(html: string, leads: readonly number[] = []): string {
  return htmlLines(html)
    .map((line, i) => {
      const lead = leads[i] ?? 0;
      return `<span class="ln"${lead > 0 ? ` style="--lead:${lead}"` : ""}>${line}\n</span>`;
    })
    .join("");
}
