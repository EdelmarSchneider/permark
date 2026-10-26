import { describe, expect, it } from "vitest";
import { htmlLines, lineSpans } from "./htmlLines";

describe("highlighted code cut into lines", () => {
  it("gives one piece per line, none after a final line break", () => {
    expect(htmlLines("a\nb\n")).toEqual(["a", "b"]);
    expect(htmlLines("a\nb")).toEqual(["a", "b"]);
    expect(htmlLines("")).toEqual([""]);
    expect(htmlLines("\n")).toEqual([""]);
    expect(htmlLines("a\n\n\nb")).toEqual(["a", "", "", "b"]);
  });

  it("closes a token at the end of its line and opens it again on the next", () => {
    const html = '<span class="c">/* one\ntwo */</span> x\n<span class="k">fn</span>';
    expect(htmlLines(html)).toEqual([
      '<span class="c">/* one</span>',
      '<span class="c">two */</span> x',
      '<span class="k">fn</span>',
    ]);
  });

  it("keeps nested tokens nested, closed in reverse order", () => {
    const html = '<span class="s">"a<span class="e">\\\n</span>b"</span>';
    expect(htmlLines(html)).toEqual([
      '<span class="s">"a<span class="e">\\</span></span>',
      '<span class="s"><span class="e"></span>b"</span>',
    ]);
  });

  it("makes no line of tags alone after the last line break", () => {
    expect(htmlLines('<span class="c"># note\n</span>')).toEqual(['<span class="c"># note</span>']);
  });

  it("drops the carriage return of CRLF text, also before closing tags", () => {
    expect(htmlLines("a\r\nb\r\n")).toEqual(["a", "b"]);
    expect(htmlLines('<span class="c">x\r</span>\ny')).toEqual(['<span class="c">x</span>', "y"]);
  });

  it("leaves an element without a line break whole and escaped text as it is", () => {
    const html = 'TOKEN=<span class="hljs-string">abc</span>\n&lt;b&gt; &amp;';
    expect(htmlLines(html)).toEqual([
      'TOKEN=<span class="hljs-string">abc</span>',
      "&lt;b&gt; &amp;",
    ]);
  });

  it("wraps every line in a span with its line break inside", () => {
    expect(lineSpans("a\n\nb")).toBe(
      '<span class="ln">a\n</span><span class="ln">\n</span><span class="ln">b\n</span>',
    );
    expect(lineSpans("")).toBe('<span class="ln">\n</span>');
  });

  it("cuts 20 000 highlighted lines quickly", () => {
    const html = Array.from({ length: 20_000 }, (_, i) => `<span class="n">${i}</span> = x`).join(
      "\n",
    );
    const startedAt = performance.now();
    expect(htmlLines(html)).toHaveLength(20_000);
    expect(performance.now() - startedAt).toBeLessThan(1000);
  });
});
