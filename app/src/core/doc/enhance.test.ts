// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { assetPathOf, enhanceBlock, languageOf, type EnhanceEnv } from "./enhance";

const env: EnhanceEnv = { assetUrl: (path) => `asset://localhost/${path}` };

function make(html: string): HTMLElement {
  const template = document.createElement("template");
  template.innerHTML = html;
  const first = template.content.firstElementChild;
  if (!(first instanceof HTMLElement)) throw new Error("no element");
  return first;
}

describe("enhanceBlock", () => {
  it("wraps a code block that is a block of its own and passes the mark on", () => {
    const root = enhanceBlock(
      make('<pre data-b="x1"><code class="language-rust">fn main() {}</code></pre>'),
      env,
    );
    expect(root.className).toBe("code-block");
    expect(root.getAttribute("data-b")).toBe("x1");
    expect(root.getAttribute("data-info")).toBe("rust");
    expect(root.querySelector("pre")?.hasAttribute("data-b")).toBe(false);
    expect(root.querySelector("pre > code")?.textContent).toBe("fn main() {}");
  });

  it("wraps code inside a list in place", () => {
    const root = enhanceBlock(make("<ul><li>a<pre><code>x</code></pre></li></ul>"), env);
    expect(root.localName).toBe("ul");
    expect(root.querySelector("li > .code-block > pre")).not.toBeNull();
    expect(root.querySelector(".code-block")?.getAttribute("data-info")).toBe("");
  });

  it("wraps tables", () => {
    const root = enhanceBlock(
      make('<table data-b="t"><tbody><tr><td>1</td></tr></tbody></table>'),
      env,
    );
    expect(root.className).toBe("table-wrap");
    expect(root.getAttribute("data-b")).toBe("t");
    expect(root.querySelector("table td")?.textContent).toBe("1");
  });

  it("gives headings an anchor link with a label", () => {
    const root = enhanceBlock(make('<h2 id="intro">Getting started</h2>'), env);
    const anchor = root.querySelector("a.anchor");
    expect(anchor?.getAttribute("href")).toBe("#intro");
    expect(anchor?.getAttribute("aria-label")).toContain("Getting started");
    // enhancing twice does not add a second one
    enhanceBlock(root, env);
    expect(root.querySelectorAll("a.anchor")).toHaveLength(1);
    // no id, no anchor
    expect(enhanceBlock(make("<h3>x</h3>"), env).querySelector("a.anchor")).toBeNull();
  });

  it("titles alerts with an icon", () => {
    const root = enhanceBlock(
      make(
        '<div class="markdown-alert markdown-alert-warning"><p class="markdown-alert-title">Warning</p><p>careful</p></div>',
      ),
      env,
    );
    const title = root.querySelector(".markdown-alert-title");
    expect(title?.querySelector("svg")).not.toBeNull();
    expect(title?.textContent).toBe("Warning");
    enhanceBlock(root, env);
    expect(root.querySelectorAll(".markdown-alert-title svg")).toHaveLength(1);
  });

  it("marks done tasks", () => {
    const root = enhanceBlock(
      make(
        '<ul class="contains-task-list"><li class="task-list-item"><input type="checkbox" checked disabled> a</li><li class="task-list-item"><input type="checkbox" disabled> b</li></ul>',
      ),
      env,
    );
    const items = root.querySelectorAll("li");
    expect(items[0]?.classList.contains("is-done")).toBe(true);
    expect(items[1]?.classList.contains("is-done")).toBe(false);
  });

  it("turns asset URLs into loadable ones and decodes the path", () => {
    const root = enhanceBlock(make('<p><img src="pm-asset:docs/img/a%20b.png" alt="x"></p>'), env);
    const img = root.querySelector("img");
    expect(img?.getAttribute("src")).toBe("asset://localhost/docs/img/a b.png");
    expect(img?.getAttribute("loading")).toBe("lazy");
    expect(root.classList.contains("pm-wide")).toBe(true);
    const video = enhanceBlock(
      make('<video src="pm-asset:v.mp4" poster="pm-asset:p.png" controls></video>'),
      env,
    );
    expect(video.getAttribute("src")).toBe("asset://localhost/v.mp4");
    expect(video.getAttribute("poster")).toBe("asset://localhost/p.png");
  });

  it("does not fetch pictures from the web: a chip with the description stands in", () => {
    const root = enhanceBlock(
      make('<p>See <img src="https://example.com/badge.svg" alt="build passing"> now</p>'),
      env,
    );
    expect(root.querySelector("img")).toBeNull();
    const chip = root.querySelector(".remote-image");
    expect(chip?.textContent).toBe("build passing");
    expect(chip?.getAttribute("data-url")).toBe("https://example.com/badge.svg");
    expect(root.classList.contains("pm-wide")).toBe(false);
    // protocol-relative too; media loses its remote source
    expect(
      enhanceBlock(make('<p><img src="//example.com/x.png" alt=""></p>'), env).querySelector(
        ".remote-image",
      ),
    ).not.toBeNull();
    const video = enhanceBlock(
      make('<video src="https://example.com/v.mp4" poster="http://x/p.png"></video>'),
      env,
    );
    expect(video.hasAttribute("src")).toBe(false);
    expect(video.hasAttribute("poster")).toBe(false);
  });

  it("does not turn a paragraph with text into a wide image paragraph", () => {
    const root = enhanceBlock(make('<p>text <img src="pm-asset:a.png" alt=""></p>'), env);
    expect(root.classList.contains("pm-wide")).toBe(false);
  });

  it("reads asset paths and languages", () => {
    expect(assetPathOf("pm-asset:a%2Fb.png")).toBe("a/b.png");
    expect(assetPathOf("pm-asset:%E0%A4%A")).toBeNull();
    expect(assetPathOf("https://x/y.png")).toBeNull();
    expect(languageOf(make('<pre><code class="language-c++ x">a</code></pre>'))).toBe("c++");
    expect(languageOf(make("<pre><code>a</code></pre>"))).toBe("");
  });

  it("survives odd blocks", () => {
    for (const html of [
      "<p></p>",
      "<hr>",
      "<div><div><div></div></div></div>",
      "<h1 id=''></h1>",
      "<table></table>",
      "<pre></pre>",
    ]) {
      expect(() => enhanceBlock(make(html), env)).not.toThrow();
    }
  });
});
