// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { commentTargetIn, quoteOf } from "./commentDom";

let body: HTMLElement;

function render(html: string): HTMLElement {
  document.body.replaceChildren();
  body = document.createElement("div");
  body.className = "md-body";
  body.innerHTML = html;
  document.body.append(body);
  return body;
}

/** Selects from the start of `from` (a text offset inside `fromNode`) to `to`. */
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
  const node = body.querySelector(selector)?.firstChild;
  if (!(node instanceof Text)) throw new Error(`no text in ${selector}`);
  return node;
};

beforeEach(() => {
  window.getSelection()?.removeAllRanges();
  // jsdom has no layout
  Range.prototype.getBoundingClientRect = () => new DOMRect(10, 20, 100, 16);
});

describe("quoteOf", () => {
  it("is the selected text", () => {
    render("<p>The quick brown fox.</p>");
    const node = text("p");
    expect(quoteOf(select(node, 4, node, 15))).toBe("quick brown");
  });

  it("puts a line break where a block ends", () => {
    render("<p>First paragraph.</p><p>Second paragraph.</p>");
    const range = select(text("p:nth-child(1)"), 6, text("p:nth-child(2)"), 6);
    expect(quoteOf(range)).toBe("paragraph.\nSecond");
  });

  it("separates the cells of a table row, and the items of a list", () => {
    render("<table><tr><td>Mon</td><td>Start</td></tr></table><ul><li>one</li><li>two</li></ul>");
    const table = quoteOf(select(text("td:nth-child(1)"), 0, text("td:nth-child(2)"), 5));
    expect(table).toBe("Mon\tStart");
    const list = quoteOf(select(text("li:nth-child(1)"), 0, text("li:nth-child(2)"), 3));
    expect(list).toBe("one\ntwo");
  });

  it("leaves out what the app drew around the document's text", () => {
    render(
      '<h2 id="a"><a class="anchor" href="#a">#</a>Rollout</h2>' +
        '<div class="code-block"><span class="code-block__lang">rust</span><button class="code-block__copy">copy</button><pre><code>let x = 1;</code></pre></div>',
    );
    const range = document.createRange();
    range.selectNodeContents(body);
    expect(quoteOf(range)).toBe("Rollout\nlet x = 1;");
  });
});

describe("commentTargetIn", () => {
  const DOC =
    '<h1 data-sourcepos="1:1-1:6">Plan</h1>' +
    '<p data-sourcepos="3:1-3:16">Intro paragraph.</p>' +
    '<h2 data-sourcepos="5:1-5:11">Rollout</h2>' +
    '<p data-sourcepos="7:1-8:20">Ship it on Friday,\nafter the review.</p>';

  it("is the selection with its lines, its headings and where it is on screen", () => {
    render(DOC);
    const node = text("p:nth-of-type(2)");
    select(node, 0, node, 11);
    const target = commentTargetIn(body);
    expect(target?.quote).toBe("Ship it on");
    expect(target?.startLine).toBe(7);
    expect(target?.endLine).toBe(8);
    expect(target?.headingPath).toEqual(["Plan", "Rollout"]);
    expect(target?.rect.top).toBe(20);
    expect(target?.range.toString()).toBe("Ship it on ");
  });

  it("is under the first heading for text above the second", () => {
    render(DOC);
    const node = text("p");
    select(node, 0, node, 5);
    expect(commentTargetIn(body)?.headingPath).toEqual(["Plan"]);
  });

  it("has no lines for blocks that do not know theirs, and the nearest heading only", () => {
    render("<h2>Rollout</h2><p>Ship it.</p>");
    const node = text("p");
    select(node, 0, node, 4);
    const target = commentTargetIn(body);
    expect(target?.startLine).toBe(0);
    expect(target?.endLine).toBe(0);
    expect(target?.headingPath).toEqual(["Rollout"]);
  });

  it("is nothing when there is no selection, an empty one, or one outside the document", () => {
    render(DOC);
    expect(commentTargetIn(body)).toBeNull();
    const node = text("p");
    select(node, 3, node, 3);
    expect(commentTargetIn(body)).toBeNull();
    const outside = document.createElement("p");
    outside.textContent = "elsewhere";
    document.body.append(outside);
    select(outside.firstChild as Text, 0, outside.firstChild as Text, 5);
    expect(commentTargetIn(body)).toBeNull();
    // only spaces
    const spaces = document.createElement("p");
    spaces.textContent = "   ";
    body.append(spaces);
    select(spaces.firstChild as Text, 0, spaces.firstChild as Text, 3);
    expect(commentTargetIn(body)).toBeNull();
  });
});
