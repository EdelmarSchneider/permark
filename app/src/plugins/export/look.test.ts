// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { componentRules, darkPreference, pageRule } from "./look";

afterEach(() => document.head.replaceChildren());

describe("page rule", () => {
  const a4 = { widthMm: 210, heightMm: 297, marginMm: 19.05 };

  it("size and margins from the page box; the printable height for figures", () => {
    const rule = pageRule(a4, false);
    expect(rule).toContain("@page { size: 210mm 297mm; margin: 19.05mm; }");
    expect(rule).toContain("--page-h: 258.9mm;");
    expect(rule).not.toContain("@bottom-center");
  });

  it("page numbers at the bottom when asked", () => {
    expect(pageRule(a4, true)).toContain(
      '@bottom-center { content: counter(page) " / " counter(pages);',
    );
  });
});

describe("dark preference", () => {
  it("the dark theme's tokens apply on the root when the reader prefers dark", () => {
    const tokens =
      ':root,\n[data-theme="light"] {\n  --bg: #fff;\n}\n[data-theme="dark"] {\n  color-scheme: dark;\n  --bg: #111;\n}\n';
    const css = darkPreference(tokens);
    expect(css).toMatch(/^@media \(prefers-color-scheme: dark\) \{\n:root \{/);
    expect(css).toContain("--bg: #111;");
    expect(css).not.toContain("#fff");
  });
});

describe("component rules", () => {
  it("only the scoped rules of components drawn on the page come along", () => {
    const style = document.createElement("style");
    style.textContent = [
      ".figure.svelte-abc { border: 1px solid; }",
      ".unused.svelte-abc { color: red; }",
      ".figure.svelte-abc::after { content: ''; }",
      ".md p { margin: 0; }",
      "@media (max-width: 100px) { .figure.svelte-abc { padding: 0; } .unused.svelte-abc { padding: 0; } }",
    ].join("\n");
    document.head.append(style);
    const root = document.createElement("main");
    root.className = "md";
    root.innerHTML = '<figure class="figure svelte-abc"></figure><p>text</p>';
    const css = componentRules(root);
    expect(css).toContain(".figure.svelte-abc { border: 1px solid; }");
    expect(css).toContain(".figure.svelte-abc::after");
    expect(css).toMatch(
      /@media \(max-width: 100px\) \{\n\.figure\.svelte-abc \{ padding: 0px?; \}\n\}/,
    );
    expect(css).not.toContain("unused");
    // global styles come from the reading view's own sheets, not from the window
    expect(css).not.toContain(".md p");
  });
});
