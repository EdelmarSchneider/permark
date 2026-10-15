// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { readableText, selectionIn } from "./domUtil";

let body: HTMLElement;

function render(html: string): HTMLElement {
  document.body.replaceChildren();
  body = document.createElement("div");
  body.className = "md-body";
  body.innerHTML = html;
  document.body.append(body);
  return body;
}

function select(startNode: Node, startOffset: number, endNode: Node, endOffset: number): Range {
  const range = document.createRange();
  range.setStart(startNode, startOffset);
  range.setEnd(endNode, endOffset);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
  return range;
}

const text = (selector: string): Text => {
  const node = body.querySelector(selector)?.lastChild;
  if (!(node instanceof Text)) throw new Error(`no text in ${selector}`);
  return node;
};

beforeEach(() => {
  window.getSelection()?.removeAllRanges();
  // jsdom has no layout
  Range.prototype.getBoundingClientRect = () => new DOMRect(10, 20, 100, 16);
});

/** The reading view puts a "#" link in front of each heading (see `enhance.ts`). */
const DOC =
  '<p data-sourcepos="1:1-1:21">Opening paragraph.</p>' +
  '<h2 id="ledger" data-sourcepos="3:1-3:10"><a class="anchor" href="#ledger">#</a>Ledger</h2>' +
  '<p data-sourcepos="5:1-5:13">Next paragraph.</p>';

describe("selectionIn", () => {
  it("does not carry the # of a heading's anchor into the text", () => {
    render(DOC);
    select(text("p:nth-of-type(1)"), 8, text("p:nth-of-type(2)"), 4);
    const selected = selectionIn(body);
    expect(selected?.text).toBe("paragraph.\nLedger\nNext");
    // the section is the one the selection starts in, and it starts above the first heading
    expect(selected?.section).toBeNull();
  });

  it("is the text of the heading when the selection starts in the anchor", () => {
    render(DOC);
    const anchor = body.querySelector(".anchor")?.firstChild as Text;
    select(anchor, 0, text("h2"), 6);
    expect(selectionIn(body)?.text).toBe("Ledger");
  });

  it("is nothing when only the anchor is selected", () => {
    render(DOC);
    const anchor = body.querySelector(".anchor")?.firstChild as Text;
    select(anchor, 0, anchor, 1);
    expect(selectionIn(body)).toBeNull();
  });

  it("is nothing outside the document, collapsed, or only spaces", () => {
    render(DOC);
    expect(selectionIn(body)).toBeNull();
    const outside = document.createElement("p");
    outside.textContent = "elsewhere";
    document.body.append(outside);
    select(outside.firstChild as Text, 0, outside.firstChild as Text, 5);
    expect(selectionIn(body)).toBeNull();
    const node = text("p");
    select(node, 3, node, 3);
    expect(selectionIn(body)).toBeNull();
  });

  it("keeps the indentation and line breaks of the code it covers", () => {
    render(
      '<div class="code-block"><span class="code-block__lang">python</span>' +
        '<button class="code-block__copy">copy</button>' +
        "<pre><code>def f():\n    return 1\n</code></pre></div>" +
        "<p>After.</p>",
    );
    // the whole block, from before its label to the paragraph after it
    select(body.querySelector(".code-block") as Node, 0, text("p"), 6);
    expect(selectionIn(body)?.text).toBe("def f():\n    return 1\nAfter.");
  });

  it("keeps the indentation of a snippet selected inside one code block", () => {
    render("<pre><code>def f():\n    return 1\n    return 2\n</code></pre>");
    const code = text("code");
    select(code, 13, code, 34);
    expect(selectionIn(body)?.text).toBe("return 1\n    return 2");
  });
});

describe("readableText", () => {
  it("tidies the spaces around line breaks, except in code when asked to keep it", () => {
    render("<ul><li>one</li><li>two</li></ul><pre>  a\n    b</pre>");
    const range = document.createRange();
    range.selectNodeContents(body);
    expect(readableText(range)).toBe("one\ntwo\na\nb");
    expect(readableText(range, true)).toBe("one\ntwo\n  a\n    b");
  });
});
