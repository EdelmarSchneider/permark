// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { enhanceBlocks, type EnhanceContext } from "./enhance";

const context: EnhanceContext = {
  docPath: "docs/a.md",
  theme: "light",
  labels: {
    copyCode: "Copy code",
    codeAuto: (name) => `auto: ${name}`,
    codeAutoHint: "Guessed from the code.",
    anchorLabel: (name) => name,
    footnoteBackref: (ref) => ref,
    diagramPending: "...",
    diagramFailed: "failed",
    alertTitles: {},
  },
  resolveImage: async (path) => path,
};

/** The block the reading view makes of a fenced block, once the libraries have loaded. */
async function drawn(html: string): Promise<HTMLElement> {
  const template = document.createElement("template");
  template.innerHTML = html;
  const { els, tasks } = enhanceBlocks(
    Array.from(template.content.children) as HTMLElement[],
    context,
  );
  await Promise.all(tasks);
  return els[0];
}

const fence = (info: string | null, code: string): string =>
  `<pre><code${info === null ? "" : ` class="language-${info}"`}>${code}\n</code></pre>`;

const label = (block: HTMLElement): HTMLElement | null =>
  block.querySelector<HTMLElement>(".code-block__lang");

describe("a fenced block in the reading view", () => {
  it("is coloured and labelled with the language's name", async () => {
    const block = await drawn(fence("python", "def hi():\n    return 1"));
    expect(block.classList.contains("code-block")).toBe(true);
    expect(label(block)?.textContent).toBe("Python");
    expect(label(block)?.title).toBe("");
    expect(block.querySelector("code")?.classList.contains("hljs")).toBe(true);
    expect(block.querySelector("code .hljs-keyword")?.textContent).toBe("def");
  });

  it("keeps the label before the copy button and the code after it", async () => {
    const block = await drawn(fence("rust", "fn main() {}"));
    const kinds = Array.from(block.children).map((child) => child.className.split(" ")[0]);
    expect(kinds).toEqual(["code-block__lang", "icon-btn", ""]);
    expect(block.children[2].tagName).toBe("PRE");
  });

  it("takes an alias, a different case, and what follows the language, for the language", async () => {
    for (const info of ["py", "PY", "python", "{python}", "{.python}"]) {
      const block = await drawn(fence(info, "import os"));
      expect(label(block)?.textContent, info).toBe("Python");
      expect(block.querySelector(".hljs-keyword"), info).not.toBeNull();
    }
    const rust = await drawn(fence("rust,ignore", "fn main() {}"));
    expect(label(rust)?.textContent).toBe("Rust");
    expect(rust.querySelector(".hljs-keyword")).not.toBeNull();
  });

  it("names the flavour the author wrote, not the grammar it shares", async () => {
    expect(label(await drawn(fence("toml", '[a]\nb = "c"')))?.textContent).toBe("TOML");
    expect(label(await drawn(fence("html", "<p>hi</p>")))?.textContent).toBe("HTML");
    expect(label(await drawn(fence("c++", "int main() {}")))?.textContent).toBe("C++");
  });

  it("shows a guessed language as a guess, and says so when hovered", async () => {
    const block = await drawn(
      fence(null, "{\n  &quot;name&quot;: &quot;permark&quot;,\n  &quot;private&quot;: true\n}"),
    );
    expect(label(block)?.textContent).toBe("auto: JSON");
    expect(label(block)?.title).toBe("Guessed from the code.");
    expect(block.querySelector(".hljs-attr")).not.toBeNull();
  });

  it("stays plain, with no label, when nothing is named and nothing is certain", async () => {
    const tree = "src/\n├── lib/\n│   └── api.ts\n└── main.ts";
    const block = await drawn(fence(null, tree));
    expect(label(block)).toBeNull();
    expect(block.querySelector("code")?.classList.contains("hljs")).toBe(false);
    expect(block.querySelector("code")?.textContent).toBe(`${tree}\n`);
  });

  it("stays plain, with no label, for the names that mean plain text", async () => {
    for (const info of ["text", "txt", "plaintext"]) {
      const block = await drawn(fence(info, "def hi():\n    return 1"));
      expect(label(block), info).toBeNull();
      expect(block.querySelector("code .hljs-keyword"), info).toBeNull();
    }
  });

  it("is labelled with the author's word when the language is not one highlight.js has", async () => {
    const block = await drawn(fence("Terraform", 'resource "x" {}'));
    expect(label(block)?.textContent).toBe("Terraform");
    expect(block.querySelector("code")?.classList.contains("hljs")).toBe(false);
  });

  it("reads a block named with an unknown word like one with no name: only a certain sign makes a guess", async () => {
    const block = await drawn(fence("zzz", "[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]"));
    // JSON that parses is certain, whatever name was written: that is the same bar as for a block with none
    expect(label(block)?.textContent).toBe("auto: JSON");
    const prose = await drawn(fence("zzz", "just some words, and some more words"));
    expect(label(prose)?.textContent).toBe("zzz");
  });

  it("keeps the code's characters when it escapes them", async () => {
    const block = await drawn(fence("python", "print(&quot;&lt;a&gt; &amp; b&quot;)"));
    expect(block.querySelector("code")?.textContent).toBe('print("<a> & b")\n');
  });

  it("draws a block it has seen before at once, with no wait for the libraries", async () => {
    await drawn(fence("python", "import sys"));
    const template = document.createElement("template");
    template.innerHTML = fence("python", "import sys");
    const { els } = enhanceBlocks(Array.from(template.content.children) as HTMLElement[], context);
    // no await: the answer of the first time is applied while the block is made
    expect(label(els[0])?.textContent).toBe("Python");
    expect(els[0].querySelector(".hljs-keyword")).not.toBeNull();
  });

  it("leaves a Mermaid diagram to its own renderer", () => {
    const template = document.createElement("template");
    template.innerHTML = fence("mermaid", "graph TD; A-->B");
    const { els } = enhanceBlocks(Array.from(template.content.children) as HTMLElement[], context);
    expect(els[0].tagName).toBe("FIGURE");
    expect(els[0].dataset.kind).toBe("mermaid");
  });

  it("leaves a PlantUML diagram to its own renderer, including the short name", () => {
    const template = document.createElement("template");
    template.innerHTML = fence("plantuml", "@startuml\nAlice -> Bob\n@enduml");
    const { els } = enhanceBlocks(Array.from(template.content.children) as HTMLElement[], context);
    expect(els[0].tagName).toBe("FIGURE");
    expect(els[0].className).toContain("diagram");
    expect(els[0].dataset.kind).toBe("plantuml");
    expect(els[0].dataset.source).toContain("Alice -> Bob");

    const short = document.createElement("template");
    short.innerHTML = fence("puml", "Alice -> Bob");
    const { els: puml } = enhanceBlocks(
      Array.from(short.content.children) as HTMLElement[],
      context,
    );
    expect(puml[0].dataset.kind).toBe("plantuml");
  });
});
