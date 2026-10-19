// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import type { CommentView, DocumentView } from "../../api";
import { documents } from "../../core/doc/document.svelte";
import { applyMarks, clearMarks, commentAt, readSelection } from "./marks";

function comment(id: string, line: number, endLine = line): CommentView {
  return {
    id,
    path: "a.md",
    text: "t",
    quote: "q",
    headingPath: [],
    line,
    endLine,
    createdMs: 1,
    state: "anchored",
    done: false,
    doneMs: null,
    doneBy: null,
    reply: null,
    replies: [],
  };
}

function page(): HTMLElement {
  document.body.innerHTML =
    '<div id="root"><h1 data-b="h">Title</h1><p data-b="p1">First paragraph.</p><p data-b="p2">Second one.</p><p data-b="p3">Third.</p></div>';
  const root = document.getElementById("root");
  if (!root) throw new Error("root");
  const view = {
    path: "a.md",
    body: {
      kind: "markdown",
      blocks: [
        { id: "h", kind: "h1", lineStart: 1, lineEnd: 1, html: null },
        { id: "p1", kind: "p", lineStart: 3, lineEnd: 3, html: null },
        { id: "p2", kind: "p", lineStart: 5, lineEnd: 5, html: null },
        { id: "p3", kind: "p", lineStart: 7, lineEnd: 7, html: null },
      ],
      outline: [],
    },
  } as unknown as DocumentView;
  vi.spyOn(documents, "view", "get").mockReturnValue(view);
  vi.spyOn(documents.host, "element").mockImplementation(
    (id) => root.querySelector<HTMLElement>(`[data-b="${id}"]`) ?? undefined,
  );
  vi.spyOn(documents.host, "container", "get").mockReturnValue(root);
  return root;
}

afterEach(() => {
  vi.restoreAllMocks();
  window.getSelection()?.removeAllRanges();
  document.body.replaceChildren();
});

describe("the bars", () => {
  it("mark the blocks a comment touches and count where several begin in one block", () => {
    const root = page();
    applyMarks(root, [comment("a", 3), comment("b", 3), comment("c", 5, 7)]);
    const marked = (id: string): HTMLElement | null => root.querySelector(`[data-b="${id}"]`);
    expect(marked("h")?.classList.contains("pm-cmt")).toBe(false);
    expect(marked("p1")?.getAttribute("data-cmt")).toBe("a b");
    expect(marked("p1")?.getAttribute("data-cmt-count")).toBe("2");
    expect(marked("p2")?.getAttribute("data-cmt")).toBe("c");
    expect(marked("p2")?.classList.contains("pm-cmt-first")).toBe(true);
    // the second block of a passage carries the bar but not the count
    expect(marked("p3")?.getAttribute("data-cmt")).toBe("c");
    expect(marked("p3")?.classList.contains("pm-cmt-first")).toBe(false);
  });

  it("are drawn again from scratch and taken off cleanly", () => {
    const root = page();
    applyMarks(root, [comment("a", 3)]);
    applyMarks(root, [comment("b", 5)]);
    expect(root.querySelectorAll(".pm-cmt")).toHaveLength(1);
    expect(root.querySelector(".pm-cmt")?.getAttribute("data-b")).toBe("p2");
    clearMarks(root);
    expect(root.querySelectorAll(".pm-cmt, [data-cmt], [data-cmt-count]")).toHaveLength(0);
  });

  it("leave a page that is not Markdown alone", () => {
    const root = page();
    vi.spyOn(documents, "view", "get").mockReturnValue(null);
    applyMarks(root, [comment("a", 3)]);
    expect(root.querySelectorAll(".pm-cmt")).toHaveLength(0);
  });
});

describe("a click in the margin", () => {
  it("names the comment when it lands left of the marked block", () => {
    const root = page();
    applyMarks(root, [comment("a", 3)]);
    const block = root.querySelector<HTMLElement>('[data-b="p1"]');
    if (!block) throw new Error("block");
    vi.spyOn(block, "getBoundingClientRect").mockReturnValue(new DOMRect(100, 0, 300, 20));
    const click = (x: number, el: HTMLElement = block): string | null => {
      let found: string | null = null;
      el.addEventListener("click", (e) => (found = commentAt(e)), { once: true });
      el.dispatchEvent(new MouseEvent("click", { clientX: x, bubbles: true }));
      return found;
    };
    expect(click(90)).toBe("a");
    expect(click(150)).toBeNull();
    // a block that carries no bar
    const other = root.querySelector<HTMLElement>('[data-b="p2"]');
    expect(click(0, other ?? block)).toBeNull();
  });
});

describe("the selection of the page", () => {
  function select(node: Node, from: number, to: number): void {
    const range = document.createRange();
    range.setStart(node, from);
    range.setEnd(node, to);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
  }

  it("is read when it lies inside the page", () => {
    const root = page();
    const text = root.querySelector('[data-b="p1"]')?.firstChild;
    if (!text) throw new Error("text");
    select(text, 0, 5);
    const found = readSelection();
    expect(found?.text).toBe("First");
    expect(found?.node).toBe(text);
    expect(found?.endNode).toBe(text);
  });

  it("is nothing when it is empty, blank, or outside the page", () => {
    const root = page();
    expect(readSelection()).toBeNull();
    const outside = document.createElement("p");
    outside.textContent = "outside the page";
    document.body.append(outside);
    select(outside.firstChild as Text, 0, 7);
    expect(readSelection()).toBeNull();
    const blank = document.createElement("p");
    blank.textContent = "   ";
    root.append(blank);
    select(blank.firstChild as Text, 0, 3);
    expect(readSelection()).toBeNull();
  });
});
