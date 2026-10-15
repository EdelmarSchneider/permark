// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { buildFileIndex, resolveCodeRef } from "../codeRefs";
import { clearCodeRefs, decorateCodeRefs, refTargetOf, type CodeRefsContext } from "./codeRefsDom";

const index = buildFileIndex([
  "README.md",
  "src/main.rs",
  "src/lib/util.ts",
  "docs/guide.md",
  "pkg/a/index.ts",
  "pkg/b/index.ts",
]);

const context: CodeRefsContext = {
  resolve: (written) => resolveCodeRef(index, "docs/guide.md", written),
  title: (path, line) => (line === null ? path : `${path} @${line}`),
  titleMany: (count) => `${count} files`,
};

let root: HTMLElement;

function render(html: string): HTMLElement {
  root = document.createElement("div");
  root.innerHTML = html;
  return root;
}

beforeEach(() => {
  document.body.replaceChildren();
});

describe("inline code", () => {
  it("becomes a reference when it names a project file", () => {
    render("<p>See <code>src/main.rs:42:7</code> now.</p>");
    decorateCodeRefs(root, context);
    const code = root.querySelector("code");
    expect(code?.classList.contains("code-ref")).toBe(true);
    expect(code?.getAttribute("role")).toBe("link");
    expect(code?.getAttribute("tabindex")).toBe("0");
    expect(code?.title).toBe("src/main.rs @42");
    expect(refTargetOf(code ?? null)).toEqual({
      path: "src/main.rs",
      candidates: [],
      line: 42,
      col: 7,
    });
    expect(root.textContent).toBe("See src/main.rs:42:7 now.");
  });

  it("works without a line", () => {
    render("<p><code>README.md</code></p>");
    decorateCodeRefs(root, context);
    expect(refTargetOf(root.querySelector("code"))).toEqual({
      path: "README.md",
      candidates: [],
      line: null,
      col: null,
    });
  });

  it("lists the candidates when the name is shared", () => {
    render("<p><code>index.ts:3</code></p>");
    decorateCodeRefs(root, context);
    const code = root.querySelector("code");
    expect(code?.title).toBe("2 files");
    expect(refTargetOf(code ?? null)).toEqual({
      path: null,
      candidates: ["pkg/a/index.ts", "pkg/b/index.ts"],
      line: 3,
      col: null,
    });
  });

  it("stays as it is when the project has no such file or the code is no path", () => {
    const html =
      "<p><code>src/missing.rs:1</code> <code>let x = 1</code> <code>version 1:2</code> <code>src/lib</code></p>";
    render(html);
    decorateCodeRefs(root, context);
    expect(root.innerHTML).toBe(html);
  });

  it("is left alone in a code block and in a link", () => {
    const html =
      '<pre><code class="language-rust">src/main.rs:42</code></pre><p><a href="x"><code>src/main.rs:42</code></a></p>';
    render(html);
    decorateCodeRefs(root, context);
    expect(root.innerHTML).toBe(html);
  });

  it("is left alone when the code holds more than text", () => {
    const html = '<p><code>src/<span class="hl">main</span>.rs:42</code></p>';
    render(html);
    decorateCodeRefs(root, context);
    expect(root.innerHTML).toBe(html);
  });
});

describe("plain text", () => {
  it("wraps a path with a line and leaves the rest of the text as it was", () => {
    render("<p>The fault is in src/main.rs:42, <em>really</em>.</p>");
    decorateCodeRefs(root, context);
    const span = root.querySelector("span.code-ref");
    expect(span?.textContent).toBe("src/main.rs:42");
    expect(span?.getAttribute("title")).toBe("src/main.rs @42");
    expect(span?.getAttribute("role")).toBe("link");
    expect(root.querySelector("p")?.innerHTML.replace(/<span [^>]*>/, "<span>")).toBe(
      "The fault is in <span>src/main.rs:42</span>, <em>really</em>.",
    );
    expect(refTargetOf(span)).toMatchObject({ path: "src/main.rs", line: 42 });
  });

  it("wraps several references in one text node", () => {
    render("<p>a src/main.rs:1 and src/lib/util.ts#L5-L9 end</p>");
    decorateCodeRefs(root, context);
    const spans = Array.from(root.querySelectorAll("span.code-ref")).map((s) => s.textContent);
    expect(spans).toEqual(["src/main.rs:1", "src/lib/util.ts#L5-L9"]);
    expect(root.textContent).toBe("a src/main.rs:1 and src/lib/util.ts#L5-L9 end");
  });

  it("needs a line: a bare path in prose is not linked, though the file exists", () => {
    const html = "<p>Open src/main.rs and README.md to see it.</p>";
    render(html);
    decorateCodeRefs(root, context);
    expect(root.innerHTML).toBe(html);
  });

  it("does not link prose that only looks like a reference", () => {
    const html =
      "<p>Version 1:2 shipped at 10:30, see https://example.com/src/main.rs:42 and C:\\x\\main.rs:42.</p>";
    render(html);
    decorateCodeRefs(root, context);
    expect(root.innerHTML).toBe(html);
  });

  it("does not link a path the project does not have", () => {
    const html = "<p>Look at src/gone.rs:42 please.</p>";
    render(html);
    decorateCodeRefs(root, context);
    expect(root.innerHTML).toBe(html);
  });

  it("links inside headings, list items, table cells and emphasis", () => {
    render(
      "<h2>Crash at src/main.rs:7</h2><ul><li>see <strong>src/main.rs:8</strong></li></ul><table><tr><td>src/main.rs:9</td></tr></table>",
    );
    decorateCodeRefs(root, context);
    expect(root.querySelectorAll("span.code-ref")).toHaveLength(3);
  });

  it("is left alone in links, buttons, code, math, diagrams and removed blocks", () => {
    const html = [
      '<p><a href="x">src/main.rs:42</a></p>',
      "<p><button>src/main.rs:42</button></p>",
      "<pre>src/main.rs:42</pre>",
      '<p><span data-math-style="inline">src/main.rs:42</span></p>',
      '<p><span class="katex">src/main.rs:42</span></p>',
      '<figure class="diagram">src/main.rs:42</figure>',
      '<div class="tm-removed"><p>src/main.rs:42</p></div>',
      "<p><kbd>src/main.rs:42</kbd></p>",
    ].join("");
    render(html);
    decorateCodeRefs(root, context);
    expect(root.innerHTML).toBe(html);
  });
});

describe("decorating again and taking the marks off", () => {
  const html =
    '<p>See <code>src/main.rs:42</code> and src/lib/util.ts:3, plus <code class="x">README.md</code>.</p>';

  it("does not nest a reference in a reference", () => {
    render(html);
    decorateCodeRefs(root, context);
    const once = root.innerHTML;
    decorateCodeRefs(root, context);
    expect(root.innerHTML).toBe(once);
  });

  it("brings the document back exactly as it was", () => {
    render(html);
    decorateCodeRefs(root, context);
    expect(root.querySelectorAll(".code-ref")).toHaveLength(3);
    clearCodeRefs(root);
    expect(root.innerHTML).toBe(html);
    // the text around the spans is one text node again
    expect(root.querySelector("p")?.childNodes).toHaveLength(5);
  });

  it("finds references the project did not have before", () => {
    render("<p>see <code>src/new.rs:2</code></p>");
    decorateCodeRefs(root, context);
    expect(root.querySelector(".code-ref")).toBeNull();
    const later = {
      ...context,
      resolve: (written: string) =>
        resolveCodeRef(buildFileIndex(["src/new.rs"]), "docs/guide.md", written),
    };
    clearCodeRefs(root);
    decorateCodeRefs(root, later);
    expect(refTargetOf(root.querySelector("code"))).toMatchObject({ path: "src/new.rs", line: 2 });
  });
});

describe("refTargetOf", () => {
  it("finds the reference from an element inside it", () => {
    render('<p><code class="code-ref" data-ref-path="a.rs" data-ref-line="3"><b>x</b></code></p>');
    expect(refTargetOf(root.querySelector("b"))).toEqual({
      path: "a.rs",
      candidates: [],
      line: 3,
      col: null,
    });
  });

  it("is null for anything else, and for marks that were damaged", () => {
    render(
      '<p>text</p><code class="code-ref">x</code><code class="code-ref" data-ref-candidates="not json">y</code><code class="code-ref" data-ref-path="a.rs" data-ref-line="0">z</code>',
    );
    expect(refTargetOf(root.querySelector("p"))).toBeNull();
    expect(refTargetOf(null)).toBeNull();
    const [plain, broken, zero] = Array.from(root.querySelectorAll("code"));
    expect(refTargetOf(plain)).toBeNull();
    expect(refTargetOf(broken)).toBeNull();
    expect(refTargetOf(zero)).toEqual({ path: "a.rs", candidates: [], line: null, col: null });
  });
});
