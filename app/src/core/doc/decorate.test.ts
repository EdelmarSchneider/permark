// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import type { LinkTarget, Result } from "../../api";
import { createMockApi } from "../../api/mock";
import { Decorator, LinkCache, type LazyEmbed } from "./decorate";

const probe: LazyEmbed = () => import("../testing/EmbedProbe.svelte");

/** A block with one fenced code block per info word. */
function block(...infos: string[]): HTMLElement {
  const root = document.createElement("div");
  for (const info of infos) {
    const code = document.createElement("div");
    code.className = "code-block";
    code.setAttribute("data-info", info);
    code.innerHTML = `<pre><code>${info} source</code></pre>`;
    root.append(code);
  }
  document.body.append(root);
  return root;
}

/** A loader that waits for `release` (a library still on its way). */
function held(fail = false): { loader: LazyEmbed; release: () => void } {
  let release: () => void = () => {};
  const gate = new Promise<void>((done) => {
    release = done;
  });
  const loader: LazyEmbed = () =>
    gate.then(() => (fail ? Promise.reject(new Error("no library")) : probe()));
  return { loader, release };
}

describe("Decorator embeds", () => {
  it("idle settles once every started embed is loaded and mounted, failures included", async () => {
    const slow = held();
    const broken = held(true);
    const decorator = new Decorator(createMockApi());
    decorator.setEmbeds((language) =>
      language === "slow" ? slow.loader : language === "broken" ? broken.loader : probe,
    );
    const root = block("quick", "slow", "broken");
    decorator.near(root);
    let settled = false;
    const idle = decorator.idle().then(() => {
      settled = true;
    });
    await new Promise((done) => setTimeout(done, 5));
    expect(settled).toBe(false);
    slow.release();
    broken.release();
    await idle;
    expect(root.querySelectorAll(".embed-probe")).toHaveLength(2);
    // the block whose library failed keeps its plain text
    expect(root.querySelector('[data-info="broken"] pre code')?.textContent).toBe("broken source");
    decorator.dispose();
  });

  it("hands a code block's title, first line number and marked lines to its embed", async () => {
    const decorator = new Decorator(createMockApi());
    decorator.setEmbeds(() => probe);
    const root = block("rust", "js");
    const [titled, plain] = root.querySelectorAll(".code-block");
    titled?.setAttribute("data-title", "main.rs");
    titled?.setAttribute("data-numbers", "10");
    titled?.setAttribute("data-lines", "1,3-5");
    decorator.near(root);
    await decorator.idle();
    const [first, second] = root.querySelectorAll(".embed-probe");
    expect(first?.getAttribute("data-title")).toBe("main.rs");
    expect(first?.getAttribute("data-start")).toBe("10");
    expect(first?.getAttribute("data-marked")).toBe("1,3-5");
    expect(plain).toBeDefined();
    for (const name of ["data-title", "data-start", "data-marked"]) {
      expect(second?.getAttribute(name), name).toBe("none");
    }
    decorator.dispose();
  });

  it("a ```math block is one display formula, drawn as code only when no plugin draws formulas", async () => {
    const mounted: string[] = [];
    const named =
      (name: string): LazyEmbed =>
      () => {
        mounted.push(name);
        return probe();
      };
    const fence = (): HTMLElement => {
      const root = document.createElement("div");
      root.innerHTML =
        '<div class="code-block" data-info="math"><pre><code class="language-math" data-math-style="display">x^2\n</code></pre></div>';
      document.body.append(root);
      return root;
    };
    const loaders = new Map([
      ["@math", named("@math")],
      ["@code", named("@code")],
    ]);
    const lookup = (language: string): LazyEmbed | undefined => loaders.get(language);
    const decorator = new Decorator(createMockApi());
    decorator.setEmbeds(lookup);
    const root = fence();
    decorator.near(root);
    await decorator.idle();
    expect(mounted).toEqual(["@math"]);
    const formula = root.querySelectorAll(".embed-probe");
    expect(formula).toHaveLength(1);
    expect(formula[0]?.textContent).toBe("x^2\n");
    expect(formula[0]?.getAttribute("data-display")).toBe("true");
    // the embeds on offer are asked again (a plugin came or went): the drawn formula stays
    decorator.setEmbeds(lookup);
    await decorator.idle();
    expect(mounted).toEqual(["@math"]);
    decorator.dispose();

    mounted.length = 0;
    const plain = new Decorator(createMockApi());
    plain.setEmbeds((language) => (language === "@code" ? named(language) : undefined));
    const code = fence();
    plain.near(code);
    await plain.idle();
    expect(mounted).toEqual(["@code"]);
    plain.dispose();
  });

  it("an embed that throws while mounting leaves its block's text and look as they were, and nothing unhandled", async () => {
    const unhandled: unknown[] = [];
    const listen = (reason: unknown): void => void unhandled.push(reason);
    process.on("unhandledRejection", listen);
    try {
      // a component that draws half of itself, then fails
      function Broken(_internals: unknown, props: { source: string }): never {
        const target = document.querySelector('[data-info="broken"]');
        target?.append(document.createTextNode(`half of ${props.source}`));
        target?.classList.add("is-drawn");
        throw new Error("broken embed");
      }
      const decorator = new Decorator(createMockApi());
      const told: (string | null)[] = [];
      decorator.mounted = (element) => told.push(element.getAttribute("data-info"));
      decorator.setEmbeds((language) =>
        language === "broken" ? () => Promise.resolve({ default: Broken }) : probe,
      );
      const root = block("broken", "fine");
      const broken = root.querySelector<HTMLElement>('[data-info="broken"]');
      decorator.near(root);
      await decorator.idle();
      await new Promise((done) => setTimeout(done, 10));
      expect(broken?.innerHTML).toBe("<pre><code>broken source</code></pre>");
      expect(broken?.className).toBe("code-block");
      expect(root.querySelectorAll(".embed-probe")).toHaveLength(1);
      expect(unhandled).toEqual([]);
      expect(told).toEqual(["fine"]);
      // taken down later: still the text it had
      decorator.dispose();
      expect(broken?.innerHTML).toBe("<pre><code>broken source</code></pre>");
    } finally {
      process.off("unhandledRejection", listen);
    }
  });

  it("tells each element an embed mounted in, never one whose library failed", async () => {
    const decorator = new Decorator(createMockApi());
    const told: (string | null)[] = [];
    decorator.mounted = (element) => told.push(element.getAttribute("data-info"));
    decorator.setEmbeds((language) =>
      language === "broken" ? () => Promise.reject(new Error("no library")) : probe,
    );
    decorator.near(block("rust", "broken", "js"));
    await decorator.idle();
    expect(told.sort()).toEqual(["js", "rust"]);
    decorator.dispose();
  });

  it("idle settles at once when nothing is loading", async () => {
    const decorator = new Decorator(createMockApi());
    await expect(decorator.idle()).resolves.toBeUndefined();
  });

  it("a forced theme reaches every embed; none forced leaves the embed to follow the page", async () => {
    const forced = new Decorator(createMockApi(), { theme: "light" });
    const plain = new Decorator(createMockApi());
    for (const decorator of [forced, plain]) decorator.setEmbeds(() => probe);
    const light = block("mermaid");
    const page = block("mermaid");
    forced.near(light);
    plain.near(page);
    await Promise.all([forced.idle(), plain.idle()]);
    expect(light.querySelector(".embed-probe")?.getAttribute("data-forced")).toBe("light");
    expect(page.querySelector(".embed-probe")?.getAttribute("data-forced")).toBe("none");
    forced.dispose();
    plain.dispose();
  });
});

describe("LinkCache", () => {
  it("drops answers asked before it forgot them (files changed meanwhile)", async () => {
    let answer: (value: Result<LinkTarget[]>) => void = () => {};
    let asked = 0;
    const api = {
      ...createMockApi(),
      resolveLinks: () => {
        asked += 1;
        return new Promise<Result<LinkTarget[]>>((done) => {
          answer = done;
        });
      },
    };
    const cache = new LinkCache(api);
    cache.forDocument("a.md");
    const stale = cache.resolve(["b.md"]);
    // the request is out once the batch is sent
    await new Promise((done) => setTimeout(done, 5));
    expect(asked).toBe(1);
    cache.forget();
    answer({ ok: true, value: [{ type: "missing", path: "b.md" }] });
    expect(await stale).toEqual([null]);
    expect(cache.peek("b.md")).toBeUndefined();
    // asked again, the answer counts
    const fresh = cache.resolve(["b.md"]);
    await new Promise((done) => setTimeout(done, 5));
    answer({ ok: true, value: [{ type: "doc", path: "b.md", anchor: null }] });
    expect(await fresh).toEqual([{ type: "doc", path: "b.md", anchor: null }]);
  });
});
