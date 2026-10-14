// Markdown for the browser mock, in both directions. The mock documents are built as HTML blocks (see
// fixtures.ts), so `htmlToMarkdown` makes the source text that Source mode edits; after a save, the saved text
// is shown by `markdownToView`, a small Markdown renderer that covers what the mock documents use. It is a
// stand-in for the backend (comrak), good enough to see an edit land in the reading view, nothing more.

import { buildBlockMap } from "../editor/blockModel";
import { hashString } from "../text";
import type { BlockKind } from "../types";
import { mockParseBlocks } from "./mockBlocks";

const unescape = (text: string): string =>
  text
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&");

const escape = (text: string): string =>
  text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

// ---- HTML to Markdown ------------------------------------------------------------------------------

function inlineMarkdown(html: string): string {
  return unescape(
    html
      .replace(/<sup class="footnote-ref"><a href="#fn-(\w+)"[^>]*>[^<]*<\/a><\/sup>/g, "[^$1]")
      .replace(/<span data-math-style="display">([\s\S]*?)<\/span>/g, "$$$$$1$$$$")
      .replace(/<span data-math-style="inline">([\s\S]*?)<\/span>/g, "$$$1$$")
      .replace(/<strong>([\s\S]*?)<\/strong>/g, "**$1**")
      .replace(/<em>([\s\S]*?)<\/em>/g, "*$1*")
      .replace(/<del>([\s\S]*?)<\/del>/g, "~~$1~~")
      .replace(/<code>([\s\S]*?)<\/code>/g, "`$1`")
      .replace(/<img [^>]*?src="([^"]*)"[^>]*?alt="([^"]*)"[^>]*>/g, "![$2]($1)")
      .replace(/<a [^>]*?href="([^"]*)"[^>]*>([\s\S]*?)<\/a>/g, "[$2]($1)"),
  );
}

function listMarkdown(html: string): string {
  const items = [...html.matchAll(/<li>([\s\S]*?)<\/li>/g)].map((m) => m[1]);
  return items
    .map((item) => {
      const task = /^<input type="checkbox"( checked="")? disabled="" \/> ([\s\S]*)$/.exec(item);
      return task
        ? `- [${task[1] ? "x" : " "}] ${inlineMarkdown(task[2])}`
        : `- ${inlineMarkdown(item)}`;
    })
    .join("\n");
}

function tableMarkdown(html: string): string {
  const rows = [...html.matchAll(/<tr>([\s\S]*?)<\/tr>/g)].map((m) =>
    [...m[1].matchAll(/<t[hd]>([\s\S]*?)<\/t[hd]>/g)].map((c) => inlineMarkdown(c[1])),
  );
  if (rows.length === 0) return "";
  const line = (cells: string[]): string => `| ${cells.join(" | ")} |`;
  return [line(rows[0]), line(rows[0].map(() => "---")), ...rows.slice(1).map(line)].join("\n");
}

/** One mock block (an HTML element) as Markdown. */
export function blockToMarkdown(html: string): string {
  const heading = /^<h([1-6])>(?:<a [^>]*><\/a>)?([\s\S]*)<\/h\1>$/.exec(html);
  if (heading) return `${"#".repeat(Number(heading[1]))} ${inlineMarkdown(heading[2])}`;
  const code = /^<pre><code(?: class="language-([^"]*)")?>([\s\S]*?)\n?<\/code><\/pre>$/.exec(html);
  if (code) return `\`\`\`${code[1] ?? ""}\n${unescape(code[2])}\n\`\`\``;
  const alert =
    /^<div class="markdown-alert markdown-alert-(\w+)"><p class="markdown-alert-title">[^<]*<\/p><p>([\s\S]*)<\/p><\/div>$/.exec(
      html,
    );
  if (alert) return `> [!${alert[1].toUpperCase()}]\n> ${inlineMarkdown(alert[2])}`;
  const quote = /^<blockquote>([\s\S]*)<\/blockquote>$/.exec(html);
  if (quote) {
    const paragraphs = [...quote[1].matchAll(/<p>([\s\S]*?)<\/p>/g)].map((m) =>
      inlineMarkdown(m[1]),
    );
    return paragraphs.map((p) => `> ${p}`).join("\n>\n");
  }
  if (html.startsWith("<ul>")) return listMarkdown(html);
  if (html.startsWith("<table>")) return tableMarkdown(html);
  const footnotes =
    /^<section class="footnotes"[^>]*><ol><li id="fn-(\w+)"><p>([\s\S]*?) <a href="#fnref/.exec(
      html,
    );
  if (footnotes) return `[^${footnotes[1]}]: ${inlineMarkdown(footnotes[2])}`;
  const paragraph = /^<p>([\s\S]*)<\/p>$/.exec(html);
  if (paragraph) return inlineMarkdown(paragraph[1]);
  return html; // a definition list, details...: raw HTML is Markdown too
}

/** The Markdown file for a mock document: front matter, then one block after another. */
export function htmlToMarkdown(blocks: string[], frontmatter?: [string, string][]): string {
  const parts: string[] = [];
  if (frontmatter && frontmatter.length > 0) {
    parts.push(`---\n${frontmatter.map(([k, v]) => `${k}: ${v}`).join("\n")}\n---`);
  }
  for (const html of blocks) parts.push(blockToMarkdown(html));
  return `${parts.join("\n\n")}\n`;
}

// ---- Markdown to a view -----------------------------------------------------------------------------

function slug(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s-]/gu, "")
    .trim()
    .replace(/\s+/g, "-");
}

/** Inline Markdown to HTML (code spans, bold, italic, strike, links, images and math). */
export function inlineHtml(text: string): string {
  const stash: string[] = [];
  const keep = (html: string): string => `\u0000${stash.push(html) - 1}\u0000`;
  let out = text
    .replace(/`([^`]+)`/g, (_, c: string) => keep(`<code>${escape(c)}</code>`))
    .replace(/\$\$([^$]+)\$\$/g, (_, m: string) =>
      keep(`<span data-math-style="display">${escape(m)}</span>`),
    )
    .replace(/\$([^$\s][^$]*)\$/g, (_, m: string) =>
      keep(`<span data-math-style="inline">${escape(m)}</span>`),
    )
    .replace(/!\[([^\]]*)\]\(([^)\s]+)[^)]*\)/g, (_, alt: string, src: string) =>
      keep(`<img src="${escape(src)}" alt="${escape(alt)}" />`),
    )
    .replace(/\[\^(\w+)\]/g, (_, id: string) =>
      keep(`<sup class="footnote-ref"><a href="#fn-${id}" id="fnref-${id}">${id}</a></sup>`),
    )
    .replace(/\[([^\]]+)\]\(([^)\s]+)[^)]*\)/g, (_, label: string, href: string) =>
      keep(`<a href="${escape(href)}">${inlineHtml(label)}</a>`),
    );
  out = escape(out)
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/(^|[^*])\*([^*\s][^*]*)\*(?!\*)/g, "$1<em>$2</em>")
    .replace(/~~([^~]+)~~/g, "<del>$1</del>");
  // eslint-disable-next-line no-control-regex
  return out.replace(/\u0000(\d+)\u0000/g, (_, i: string) => stash[Number(i)]);
}

const withPos = (html: string, from: number, to: number): string =>
  html.replace(/^<(\w+)/, `<$1 data-sourcepos="${from + 1}:1-${to + 1}:1"`);

function listHtml(text: string): string {
  const lines = text.split(/\r\n|\r|\n/).filter((l) => l.trim() !== "");
  const ordered = /^\s*\d/.test(lines[0] ?? "");
  const items = lines.map((line) => {
    const body = line.replace(/^\s*(?:[-+*]|\d+[.)])\s?/, "");
    const task = /^\[([ xX])\] (.*)$/.exec(body);
    return task
      ? `<li><input type="checkbox"${task[1] === " " ? "" : ' checked=""'} disabled="" /> ${inlineHtml(task[2])}</li>`
      : `<li>${inlineHtml(body)}</li>`;
  });
  return `<${ordered ? "ol" : "ul"}>${items.join("")}</${ordered ? "ol" : "ul"}>`;
}

function tableHtml(text: string): string {
  const rows = text
    .split(/\r\n|\r|\n/)
    .filter((l) => !/^\s*\|?[\s:|-]+\|?\s*$/.test(l) || !l.includes("-"))
    .map((l) =>
      l
        .trim()
        .replace(/^\||\|$/g, "")
        .split("|")
        .map((c) => inlineHtml(c.trim())),
    );
  if (rows.length === 0) return "";
  const head = rows[0].map((c) => `<th>${c}</th>`).join("");
  const body = rows
    .slice(1)
    .map((r) => `<tr>${r.map((c) => `<td>${c}</td>`).join("")}</tr>`)
    .join("");
  return `<table><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table>`;
}

function blockHtml(kind: BlockKind, text: string): string {
  const lines = text.split(/\r\n|\r|\n/);
  switch (kind) {
    case "heading": {
      const atx = /^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$/.exec(lines[0]);
      const level = atx ? atx[1].length : /^ {0,3}=/.test(lines[lines.length - 1]) ? 1 : 2;
      const label = atx ? atx[2] : lines.slice(0, -1).join(" ");
      const id = slug(label);
      return `<h${level}><a href="#${id}" aria-hidden="true" class="anchor" id="${id}"></a>${inlineHtml(label)}</h${level}>`;
    }
    case "code": {
      const open = /^ {0,3}(`{3,}|~{3,})\s*(\S*)/.exec(lines[0]);
      if (!open)
        return `<pre><code>${escape(lines.map((l) => l.replace(/^ {4}|^\t/, "")).join("\n"))}\n</code></pre>`;
      const body = lines.slice(
        1,
        /^ {0,3}(`{3,}|~{3,})\s*$/.test(lines[lines.length - 1]) ? -1 : undefined,
      );
      const lang = open[2] ? ` class="language-${escape(open[2])}"` : "";
      return `<pre><code${lang}>${escape(body.join("\n"))}\n</code></pre>`;
    }
    case "list":
      return listHtml(text);
    case "table":
      return tableHtml(text);
    case "blockquote": {
      const inner = lines.map((l) => l.replace(/^ {0,3}>\s?/, ""));
      return `<blockquote><p>${inlineHtml(inner.join(" "))}</p></blockquote>`;
    }
    case "other": {
      const alert = /^ {0,3}>\s*\[!(\w+)\]/.exec(lines[0]);
      if (alert) {
        const kind = alert[1].toLowerCase();
        const inner = lines.slice(1).map((l) => l.replace(/^ {0,3}>\s?/, ""));
        return `<div class="markdown-alert markdown-alert-${kind}"><p class="markdown-alert-title">${kind[0].toUpperCase()}${kind.slice(1)}</p><p>${inlineHtml(inner.join(" "))}</p></div>`;
      }
      return `<p>${inlineHtml(lines.join(" "))}</p>`;
    }
    case "thematicBreak":
      return "<hr />";
    case "html":
      return text;
    case "math":
      return `<p><span data-math-style="display">${escape(text.replace(/^\s*\$\$|\$\$\s*$/g, "").trim())}</span></p>`;
    case "footnoteDef":
      return `<p>${inlineHtml(text)}</p>`;
    case "frontmatter":
      return "";
    default:
      return `<p>${inlineHtml(lines.join(" "))}</p>`;
  }
}

export type MockView = {
  html: string;
  blocks: string[];
  title: string | null;
  frontmatter: [string, string][] | null;
};

/** The reading view of some Markdown text, as the backend would render it (roughly). */
export function markdownToView(text: string): MockView {
  const map = buildBlockMap(text, mockParseBlocks(text));
  const html: string[] = [];
  const blocks: string[] = [];
  let frontmatter: [string, string][] | null = null;
  let title: string | null = null;
  for (const block of map.blocks) {
    if (block.kind === "frontmatter") {
      const lines = block.text.split(/\r\n|\r|\n/).slice(1, -1);
      frontmatter = lines
        .map((l) => /^([\w-]+):\s*(.*)$/.exec(l))
        .filter((m): m is RegExpExecArray => m !== null)
        .map((m) => [m[1], m[2]] as [string, string]);
      continue;
    }
    const element = blockHtml(block.kind, block.text);
    if (element === "") continue;
    if (title === null && block.kind === "heading")
      title = /<\/a>([\s\S]*)<\/h/.exec(element)?.[1] ?? null;
    html.push(withPos(element, block.startLine, block.endLine));
    blocks.push(hashString(element));
  }
  return {
    html: html.join("\n"),
    blocks,
    title: title === null ? null : unescape(title.replace(/<[^>]+>/g, "")),
    frontmatter: frontmatter && frontmatter.length > 0 ? frontmatter : null,
  };
}
