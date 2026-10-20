// @vitest-environment jsdom
import "./jsdomShims";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { TextSelection } from "@milkdown/kit/prose/state";
import { formatOf, run } from "./commands";
import { createCore, type Core } from "./core";

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
});
