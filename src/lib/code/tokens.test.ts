import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { STYLED_SCOPES, tokenRanges } from "./tokens";

describe("the ranges of highlight.js's HTML", () => {
  it("gives each coloured stretch its place in the text", () => {
    const html =
      '<span class="hljs-keyword">fn</span> main() { <span class="hljs-number">1</span> }';
    const text = "fn main() { 1 }";
    const ranges = tokenRanges(html);
    expect(ranges).toEqual([
      { from: 0, to: 2, className: "hljs-keyword" },
      { from: 12, to: 13, className: "hljs-number" },
    ]);
    expect(text.slice(ranges[0].from, ranges[0].to)).toBe("fn");
    expect(text.slice(ranges[1].from, ranges[1].to)).toBe("1");
  });

  it("counts an escaped character as the one character it stands for", () => {
    const html = 'a &lt; b &amp;&amp; <span class="hljs-string">&quot;x&quot;&#x27;</span>';
    const text = `a < b && "x"'`;
    const [range] = tokenRanges(html);
    expect(text.slice(range.from, range.to)).toBe(`"x"'`);
  });

  it("colours a stretch by the innermost scope that has a colour", () => {
    // a template literal: the string is a string, and the `${x}` inside it is text of the string with its own scope that has no colour
    const html = '<span class="hljs-string">`a <span class="hljs-subst">${x}</span> b`</span>';
    expect(tokenRanges(html)).toEqual([{ from: 0, to: 10, className: "hljs-string" }]);
    // a doc tag inside a comment is a keyword, the rest of the comment stays a comment
    const comment =
      '<span class="hljs-comment">/** <span class="hljs-doctag">@param</span> x */</span>';
    expect(tokenRanges(comment)).toEqual([
      { from: 0, to: 4, className: "hljs-comment" },
      { from: 4, to: 10, className: "hljs-doctag" },
      { from: 10, to: 15, className: "hljs-comment" },
    ]);
  });

  it("keeps all the classes of the scope it takes, for the rules that need two", () => {
    const html = '<span class="hljs-title class_">Foo</span>';
    expect(tokenRanges(html)).toEqual([{ from: 0, to: 3, className: "hljs-title class_" }]);
  });

  it("gives no range to text outside every coloured scope", () => {
    expect(tokenRanges("plain text")).toEqual([]);
    expect(tokenRanges('<span class="hljs-subst">x</span>')).toEqual([]);
    expect(tokenRanges("")).toEqual([]);
  });

  it("joins neighbouring stretches of the same class", () => {
    const html = '<span class="hljs-keyword">a</span><span class="hljs-keyword">b</span>';
    expect(tokenRanges(html)).toEqual([{ from: 0, to: 2, className: "hljs-keyword" }]);
  });
});

describe("the scopes the style sheet colours", () => {
  it("are the ones the ranges are made from", () => {
    const css = readFileSync(new URL("../../styles/markdown.css", import.meta.url), "utf8");
    const used = new Set(css.match(/\.hljs-[\w-]+/g)?.map((selector) => selector.slice(1)));
    expect([...used].sort()).toEqual([...STYLED_SCOPES].sort());
  });
});
