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

  it("matches inside nested inline elements, one node at a time", () => {
    const root = page("<p>one <b>two</b> three</p>");
    expect(texts(findIn(root, "two"))).toEqual(["two"]);
    // a match across two nodes is not found (documented limit)
    expect(findIn(root, "one two").ranges).toEqual([]);
  });

  it("is fast on a large page", () => {
    const root = page("<p>lorem ipsum dolor sit amet</p>".repeat(20_000));
    const started = Date.now();
    const found = findIn(root, "dolor");
    expect(found.ranges).toHaveLength(5000);
    expect(found.capped).toBe(true);
    expect(Date.now() - started).toBeLessThan(3000);
  });
});
