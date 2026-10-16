// @vitest-environment jsdom
import { TextSelection } from "@milkdown/kit/prose/state";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { ensureLanguage } from "../../code/highlighter";
import { mockParseBlocks } from "../../mock/mockBlocks";
import { buildBlockMap } from "../blockModel";
import { VisualEditor } from "./editor";
import "./jsdomShims";

let editor: VisualEditor;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.appendChild(host);
  editor = await VisualEditor.create({
    host,
    label: "test",
    scroll: false,
    imageUrl: (src) => `asset://${src}`,
    locked: {
      render: (body, atom) => {
        body.textContent = atom.source;
      },
      edit: () => {},
      link: () => {},
      labels: { edit: "Edit in Source", editHint: "", region: (kind) => kind, reason: () => "" },
    },
    handlers: {
      changed: () => {},
      files: () => {},
      editInSource: () => {},
      link: () => {},
      slash: () => {},
      slashKey: () => false,
    },
  });
});
afterAll(async () => {
  await editor.destroy();
});

const load = (text: string): void => {
  editor.load({ map: buildBlockMap(text, mockParseBlocks(text)) });
};

/** The caret inside the n-th top-level node. */
function caret(index: number): void {
  const { view } = editor.core;
  let pos = 0;
  for (let i = 0; i < index; i++) pos += view.state.doc.child(i).nodeSize;
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, pos + 1)));
}

/** The lines of two texts that differ, as [before, after] pairs. */
function changedLines(before: string, after: string): [string, string][] {
  const [a, b] = [before.split("\n"), after.split("\n")];
  expect(b.length).toBe(a.length);
  return a.flatMap((line, i): [string, string][] => (line === b[i] ? [] : [[line, b[i]]]));
}

const DOC = [
  "# Notes",
  "",
  "```python",
  "def hi():",
  "    return 1",
  "```",
  "",
  "Between the blocks.",
  "",
  "~~~",
  "plain text in a tilde fence",
  "~~~",
  "",
  "````md",
  "```js",
  "inner",
  "```",
  "````",
  "",
  "Last paragraph.",
  "",
].join("\n");

describe("changing the language of a code block in Visual mode", () => {
  it("rewrites only the opening fence line of that block", () => {
    load(DOC);
    expect(editor.text()).toBe(DOC);
    caret(1);
    expect(editor.format().language).toBe("python");
    editor.setLanguage("rust");
    expect(editor.format().language).toBe("rust");
    expect(changedLines(DOC, editor.text())).toEqual([["```python", "```rust"]]);
    expect(editor.status().dirty).toBe(true);
  });

  it("keeps the fence a block was written with (tildes, four backticks)", () => {
    load(DOC);
    caret(3);
    editor.setLanguage("txt");
    expect(changedLines(DOC, editor.text())).toEqual([["~~~", "~~~txt"]]);
    load(DOC);
    caret(4);
    expect(editor.format().language).toBe("md");
    editor.setLanguage("markdown");
    expect(changedLines(DOC, editor.text())).toEqual([["````md", "````markdown"]]);
  });

  it("adds a language to a block that had none, and takes it away again", () => {
    load("```\nplain\n```\n");
    caret(0);
    editor.setLanguage("yaml");
    expect(editor.text()).toBe("```yaml\nplain\n```\n");
    editor.setLanguage("");
    expect(editor.text()).toBe("```\nplain\n```\n");
    expect(editor.status().dirty).toBe(false);
  });

  it("writes exactly what was typed after the backticks, for any name", () => {
    load("```js\nx\n```\n");
    caret(0);
    editor.setLanguage("Terraform-HCL");
    expect(editor.text()).toBe("```Terraform-HCL\nx\n```\n");
    editor.setLanguage("c#");
    expect(editor.text()).toBe("```c#\nx\n```\n");
  });

  it("leaves out of the name what an info string cannot hold", () => {
    load("```js\nx\n```\n");
    caret(0);
    editor.setLanguage("a`b\nc");
    expect(editor.text()).toBe("```a b c\nx\n```\n");
  });

  it("is undone by putting the language back", () => {
    load(DOC);
    caret(1);
    editor.setLanguage("go");
    expect(editor.status().dirty).toBe(true);
    editor.setLanguage("python");
    expect(editor.text()).toBe(DOC);
    expect(editor.status().dirty).toBe(false);
  });

  it("rewrites the whole block, as the editor writes it, once its code is edited too", () => {
    load("~~~sh\necho a\n~~~\n");
    caret(0);
    editor.setLanguage("bash");
    const { view } = editor.core;
    view.dispatch(view.state.tr.insertText("echo b\n", 1));
    expect(editor.text()).toBe("```bash\necho b\necho a\n```\n");
  });

  it("does not touch the blocks around it", () => {
    load(DOC);
    caret(1);
    editor.setLanguage("rust");
    const text = editor.text();
    expect(text.startsWith("# Notes\n\n```rust\n")).toBe(true);
    expect(text.endsWith("Last paragraph.\n")).toBe(true);
    expect(text).toContain("~~~\nplain text in a tilde fence\n~~~");
  });
});

describe("highlighting in Visual mode", () => {
  /** What is drawn in the code blocks of the editor: the classes of the coloured stretches, and the text they cover. */
  function drawn(): { className: string; text: string }[] {
    return Array.from(editor.core.view.dom.querySelectorAll("pre code span")).map((span) => ({
      className: span.className,
      text: span.textContent ?? "",
    }));
  }
  const nameOfFirstBlock = (): string | null =>
    editor.core.view.dom.querySelector("pre")?.getAttribute("data-code-name") ?? null;

  it("colours a block that names a language once its grammar has loaded", async () => {
    load("```python\ndef hi():\n    return 1\n```\n");
    await ensureLanguage("python");
    expect(nameOfFirstBlock()).toBe("Python");
    expect(drawn()).toContainEqual({ className: "hljs-keyword", text: "def" });
    expect(drawn()).toContainEqual({ className: "hljs-keyword", text: "return" });
  });

  it("reads an alias in any case, like the reading view", async () => {
    load("```PY\nimport os\n```\n");
    await ensureLanguage("python");
    expect(drawn()).toContainEqual({ className: "hljs-keyword", text: "import" });
    expect(nameOfFirstBlock()).toBe("Python");
  });

  it("shows a block with attributes after the language as it is read, not as text to edit", () => {
    const report = editor.load({
      map: buildBlockMap(
        '```rust title="main.rs"\nfn main() {}\n```\n',
        mockParseBlocks('```rust title="main.rs"\nfn main() {}\n```\n'),
      ),
    });
    expect(report.locked.differs).toBe(1);
    expect(editor.core.view.dom.querySelector("pre")).toBeNull();
  });

  it("follows the text as it is edited", async () => {
    load("```rust\nlet x = 1;\n```\n");
    await ensureLanguage("rust");
    expect(drawn().some((d) => d.text === "let")).toBe(true);
    const { view } = editor.core;
    view.dispatch(view.state.tr.insertText("// ", 1));
    expect(drawn().some((d) => d.text === "let")).toBe(false);
    expect(drawn().some((d) => d.className === "hljs-comment")).toBe(true);
  });

  it("follows a change of language", async () => {
    load("```python\nreturn 1\n```\n");
    await ensureLanguage("python");
    caret(0);
    editor.setLanguage("txt");
    expect(drawn()).toEqual([]);
    expect(nameOfFirstBlock()).toBeNull();
    editor.setLanguage("rust");
    await ensureLanguage("rust");
    expect(nameOfFirstBlock()).toBe("Rust");
  });

  it("labels a language it cannot colour with the name as written, and colours nothing", () => {
    load("```Terraform\nresource {}\n```\n");
    expect(drawn()).toEqual([]);
    expect(nameOfFirstBlock()).toBe("Terraform");
  });

  it("leaves a block with no language, and a plain-text one, alone", () => {
    load("```\ndef hi():\n```\n\n```text\ndef hi():\n```\n");
    expect(drawn()).toEqual([]);
    expect(editor.core.view.dom.querySelector("pre[data-code-name]")).toBeNull();
  });

  it("does not change what is saved", async () => {
    const text = "```python\ndef hi():\n    return 1\n```\n";
    load(text);
    await ensureLanguage("python");
    expect(editor.text()).toBe(text);
    expect(editor.status().dirty).toBe(false);
  });
});
