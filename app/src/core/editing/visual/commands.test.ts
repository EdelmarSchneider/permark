// @vitest-environment jsdom
import "./jsdomShims";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { NodeSelection, TextSelection } from "@milkdown/kit/prose/state";
import { formatOf, run } from "./commands";
import { LOCKED, createCore, type Core } from "./core";

let core: Core;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.append(host);
  core = await createCore({ host });
});

afterAll(async () => {
  await core.destroy();
});

function load(markdown: string): void {
  core.setDoc(core.parse(markdown));
}

/** Puts the caret right after the first occurrence of `text`. */
function caretAfter(text: string): void {
  const view = core.view;
  let at = -1;
  view.state.doc.descendants((node, pos) => {
    if (at >= 0 || !node.isText) return at < 0;
    const index = (node.text ?? "").indexOf(text);
    if (index >= 0) at = pos + index + text.length;
    return true;
  });
  if (at < 0) throw new Error(`no ${text}`);
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, at)));
}

const markdown = (): string => core.serialize(core.view.state.doc).trim();

describe("Edit-mode commands", () => {
  it("outdents a nested list item one level (the old bug: no way back out)", () => {
    load("- one\n  - two\n  - three\n");
    caretAfter("two");
    expect(formatOf(core.view.state).list).toBe("bullet");
    expect(run(core, { kind: "outdent" })).toBe(true);
    expect(markdown()).toBe("- one\n- two\n  - three");
  });

  it("outdents a nested ordered and task item too, and indents back", () => {
    load("1. one\n   1. two\n");
    caretAfter("two");
    run(core, { kind: "outdent" });
    expect(markdown()).toBe("1. one\n2. two");
    run(core, { kind: "indent" });
    expect(markdown()).toBe("1. one\n   1. two");
    load("- [ ] a\n  - [x] b\n");
    caretAfter("b");
    run(core, { kind: "outdent" });
    expect(markdown()).toBe("- [ ] a\n- [x] b");
  });

  it("an outdented top-level item becomes a paragraph", () => {
    load("- only\n");
    caretAfter("only");
    run(core, { kind: "outdent" });
    expect(markdown()).toBe("only");
  });

  it("turns a paragraph into a heading, a list, a task list and back", () => {
    load("words\n");
    caretAfter("words");
    run(core, { kind: "block", to: "h2" });
    expect(markdown()).toBe("## words");
    expect(formatOf(core.view.state).block).toBe("h2");
    run(core, { kind: "block", to: "paragraph" });
    run(core, { kind: "taskList" });
    expect(markdown()).toBe("- [ ] words");
    run(core, { kind: "taskList" });
    expect(markdown()).toBe("- words");
    run(core, { kind: "orderedList" });
    expect(markdown()).toBe("1. words");
  });

  it("Edit mode draws a real checkbox for a task item, named by its task, and a click flips it", () => {
    load("- [ ] a\n- [x] b\n");
    const boxes = Array.from(
      core.view.dom.querySelectorAll("li[data-item-type=task] input[type=checkbox]"),
    ) as HTMLInputElement[];
    expect(boxes.length).toBe(2);
    const [first, done] = boxes;
    expect(first?.checked).toBe(false);
    expect(done?.checked).toBe(true);
    expect(first?.getAttribute("aria-label")).toBe("a");
    const items = Array.from(core.view.dom.querySelectorAll("li[data-item-type=task]"));
    expect(items.map((li) => li.classList.contains("task-list-item"))).toEqual([true, true]);
    expect(items[1]?.classList.contains("is-done")).toBe(true);
    first?.click();
    expect(markdown()).toBe("- [x] a\n- [x] b");
    expect(boxes[0]?.checked).toBe(true);
    // a plain bullet item keeps no box
    load("- plain\n");
    expect(core.view.dom.querySelectorAll("li input[type=checkbox]").length).toBe(0);
  });

  it("toggles marks, makes and removes links, inserts a table and a rule", () => {
    load("plain text\n");
    const view = core.view;
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 1, 6)));
    run(core, { kind: "strong" });
    expect(markdown()).toBe("**plain** text");
    run(core, { kind: "link", href: "docs/a.md" });
    expect(markdown()).toMatch(
      /^(\*\*\[plain\]\(docs\/a\.md\)\*\*|\[\*\*plain\*\*\]\(docs\/a\.md\)) text$/,
    );
    expect(formatOf(view.state).link).toBe("docs/a.md");
    run(core, { kind: "link", href: null });
    expect(markdown()).toBe("**plain** text");
    caretAfter("text");
    run(core, { kind: "rule" });
    expect(markdown()).toContain("---");
    run(core, { kind: "table" });
    expect(markdown()).toMatch(/\|.*\|/);
  });

  it("changes the language of a code block", () => {
    load("```js\nlet a = 1;\n```\n");
    caretAfter("let");
    expect(formatOf(core.view.state).language).toBe("js");
    run(core, { kind: "language", language: "ts" });
    expect(markdown()).toBe("```ts\nlet a = 1;\n```");
  });

  it("draws a code block in the reading view's wrapper, the fence language as its label, the code editable", () => {
    load("```rust\nfn main() {}\n```\n\n```\nplain\n```\n");
    const blocks = Array.from(core.view.dom.querySelectorAll(":scope > .code-block"));
    expect(blocks.length).toBe(2);
    const [rust, plain] = blocks;
    expect(rust?.querySelector(":scope > pre > code")?.textContent).toBe("fn main() {}");
    expect(rust?.querySelector(".lang")?.textContent).toBe("rust");
    // no language: no label, as in the reading view
    expect(plain?.querySelector(".lang")).toBeNull();
    // the label is not part of the text
    expect(markdown()).toBe("```rust\nfn main() {}\n```\n\n```\nplain\n```");
    caretAfter("fn");
    run(core, { kind: "language", language: "ts" });
    expect(core.view.dom.querySelector(".code-block .lang")?.textContent).toBe("ts");
    caretAfter("main");
    core.view.dispatch(core.view.state.tr.insertText("2"));
    expect(markdown()).toBe("```ts\nfn main2() {}\n```\n\n```\nplain\n```");
    run(core, { kind: "language", language: "" });
    expect(core.view.dom.querySelectorAll(".code-block .lang").length).toBe(0);
  });

  it("draws a table in the reading view's wrapper, its cells editable", () => {
    load("| a | b |\n| - | - |\n| 1 | 2 |\n");
    const wrap = core.view.dom.querySelector(":scope > .table-wrap");
    const header = wrap?.querySelector(":scope > table > tbody > tr[data-is-header] > th");
    expect(header?.textContent).toBe("a");
    caretAfter("1");
    core.view.dispatch(core.view.state.tr.insertText("0"));
    expect(markdown()).toMatch(/\| 10 +\| 2 +\|/);
  });

  it("inserts a picked character at the caret, and refuses on a locked block", () => {
    load("one two\n");
    caretAfter("one ");
    expect(run(core, { kind: "text", text: "→" })).toBe(true);
    expect(markdown()).toBe("one →two");
    expect(run(core, { kind: "text", text: "" })).toBe(false);
    expect(markdown()).toBe("one →two");
    // the caret on a locked atom: the same command does nothing
    const view = core.view;
    const locked = view.state.schema.nodes[LOCKED]?.create({
      source: "<b>x</b>",
      kind: "html",
      reason: "html",
    });
    if (!locked) throw new Error("no locked node in the schema");
    view.dispatch(view.state.tr.replaceWith(0, view.state.doc.content.size, locked));
    view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, 0)));
    expect(formatOf(view.state).locked).toBe(true);
    expect(run(core, { kind: "text", text: "→" })).toBe(false);
    expect(markdown()).toBe("<b>x</b>");
  });
});
