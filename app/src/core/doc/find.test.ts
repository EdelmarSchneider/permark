// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { findIn } from "./find";

function page(html: string): HTMLElement {
  document.body.innerHTML = `<div id="root">${html}</div>`;
  const root = document.getElementById("root");
  if (!root) throw new Error("no root");
  return root;
}

const texts = (found: { ranges: Range[] }): string[] => found.ranges.map((r) => r.toString());

beforeEach(() => {
  document.body.replaceChildren();
});

describe("findIn", () => {
  it("finds every match, case-insensitively, keeping the written case", () => {
    const root = page("<p>Find me, find ME and FIND</p><p>nothing</p><p>again find</p>");
    const found = findIn(root, "find");
    expect(texts(found)).toEqual(["Find", "find", "FIND", "find"]);
    expect(found.capped).toBe(false);
  });

  it("finds matches next to each other and overlapping starts once", () => {
    const root = page("<p>aaaa</p>");
    expect(texts(findIn(root, "aa"))).toEqual(["aa", "aa"]);
  });

  it("skips scripts, styles, hidden parts", () => {
    const root = page(
      "<p>word</p><script>word</script><style>.word { color: red }</style><div hidden>word</div><template>word</template><p>Word</p>",
    );
    expect(texts(findIn(root, "word"))).toEqual(["word", "Word"]);
  });

  it("an empty query finds nothing; a missing one too", () => {
    const root = page("<p>abc</p>");
    expect(findIn(root, "").ranges).toEqual([]);
    expect(findIn(root, "zzz").ranges).toEqual([]);
  });

  it("stops at the limit and says so", () => {
    const root = page(`<p>${"x ".repeat(100)}</p>`);
    const found = findIn(root, "x", 10);
    expect(found.ranges).toHaveLength(10);
    expect(found.capped).toBe(true);
    expect(findIn(root, "x", 100).capped).toBe(false);
  });

  it("handles text whose lower case has another length", () => {
    const root = page("<p>İstanbul and istanbul</p>");
    expect(() => findIn(root, "istanbul")).not.toThrow();
    expect(findIn(root, "and").ranges).toHaveLength(1);
  });

  it("stays case-insensitive in a text with a dotted İ, and the dotted and dotless i match each other", () => {
    // "İ".toLowerCase() is two characters: offsets after it would shift
    const root = page("<p>İSTANBUL, İstanbul ve Işık; IŞIK and ışık</p>");
    expect(texts(findIn(root, "istanbul"))).toEqual(["İSTANBUL", "İstanbul"]);
    expect(texts(findIn(root, "İstanbul"))).toEqual(["İSTANBUL", "İstanbul"]);
    expect(texts(findIn(root, "ve"))).toEqual(["ve"]);
    expect(texts(findIn(root, "ışık"))).toEqual(["Işık", "IŞIK", "ışık"]);
    expect(texts(findIn(root, "IŞIK"))).toEqual(["Işık", "IŞIK", "ışık"]);
  });

  it("matches inside nested inline elements and across them, as the editor does", () => {
    const root = page("<p>one <b>two</b> three</p>");
    expect(texts(findIn(root, "two"))).toEqual(["two"]);
    const found = findIn(root, "one two");
    expect(texts(found)).toEqual(["one two"]);
    const [range] = found.ranges;
    expect(range?.startContainer.textContent).toBe("one ");
    expect(range?.endContainer.textContent).toBe("two");
    expect(texts(findIn(root, "e two t"))).toEqual(["e two t"]);
    expect(texts(findIn(root, "one two three"))).toEqual(["one two three"]);
  });

  it("finds a phrase across links, inline code and highlighted code tokens", () => {
    const root = page(
      `<p>see <a href="#x">the <em>guide</em></a> and <code>npm run</code> now</p>` +
        `<pre><code><span class="tok-keyword">const</span> <span class="tok-name">value</span> = <span class="tok-number">1</span>;</code></pre>`,
    );
    expect(texts(findIn(root, "see the guide and npm"))).toEqual(["see the guide and npm"]);
    expect(texts(findIn(root, "const value = 1;"))).toEqual(["const value = 1;"]);
    expect(texts(findIn(root, "VALUE ="))).toEqual(["value ="]);
  });

  it("never joins the text of two blocks", () => {
    const root = page(
      "<p>one</p><p>two</p>" +
        "<ul><li>three<ul><li>four</li></ul>five</li></ul>" +
        "<table><tr><td>six</td><td>seven</td></tr></table>" +
        "<h2>eight</h2><blockquote>nine<p>ten</p></blockquote>" +
        "<dl><dt>eleven</dt><dd>twelve</dd></dl>" +
        "<figure><img alt=''><figcaption>thirteen</figcaption></figure><div>fourteen</div>",
    );
    for (const query of [
      "onetwo",
      "threefour",
      "fourfive",
      "sixseven",
      "eightnine",
      "nineten",
      "eleventwelve",
      "thirteenfourteen",
    ]) {
      expect(findIn(root, query).ranges, query).toEqual([]);
    }
    expect(texts(findIn(root, "five"))).toEqual(["five"]);
  });

  it("a line break and a drawing's separate labels end the run", () => {
    const root = page(
      `<p>one<br>two</p><div><svg><text>Start</text><text><tspan>En</tspan><tspan>d</tspan></text></svg></div>`,
    );
    expect(findIn(root, "onetwo").ranges).toEqual([]);
    expect(findIn(root, "tEn").ranges).toEqual([]);
    expect(texts(findIn(root, "end"))).toEqual(["End"]);
    // after the break the run goes on through inline elements
    expect(texts(findIn(page("<p>a<br>b <b>c</b></p>"), "b c"))).toEqual(["b c"]);
  });

  it("keeps the limit over joined text", () => {
    const root = page(`<p>${"<b>x</b> ".repeat(50)}</p><p>${"x ".repeat(50)}</p>`);
    const found = findIn(root, "x x", 30);
    expect(found.ranges).toHaveLength(30);
    expect(found.capped).toBe(true);
    expect(findIn(root, "x x").ranges).toHaveLength(50);
  });

  it("is fast on a large page", () => {
    const root = page("<p>lorem ipsum dolor sit amet</p>".repeat(20_000));
    const started = Date.now();
    const found = findIn(root, "dolor");
    expect(found.ranges).toHaveLength(5000);
    expect(found.capped).toBe(true);
    // linear: ~0.4 s alone; quadratic work on 20 000 paragraphs would take minutes, the bound leaves room for load
    const took = Date.now() - started;
    expect(took, `found in ${took} ms`).toBeLessThan(10_000);
  });

  it("is fast on a large page of inline runs", () => {
    const root = page(
      "<p>lorem <b>ipsum</b> <a href='#'>dolor</a> <code>sit</code> amet</p>".repeat(10_000),
    );
    const started = Date.now();
    const found = findIn(root, "dolor sit");
    expect(found.ranges).toHaveLength(5000);
    expect(found.capped).toBe(true);
    const took = Date.now() - started;
    expect(took, `found in ${took} ms`).toBeLessThan(10_000);
  });
});
