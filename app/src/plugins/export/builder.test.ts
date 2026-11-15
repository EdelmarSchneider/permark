// @vitest-environment jsdom
import "../../core/testing/media";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Api, Block, DocumentView, Result } from "../../api";
import type { PageBox } from "../../api/generated/PageBox";
import { createMockApi } from "../../api/mock";
import type { EmbedLookup } from "../../core/doc/decorate";
import { webPictures } from "../../core/doc/webPictures.svelte";
import { buildExport, type ExportOptions, type ExportOutcome } from "./builder";

const A5: PageBox = { widthMm: 148, heightMm: 210, marginMm: 12.7 };

function value<T>(result: Result<T>): T {
  if (!result.ok) throw new Error("expected a value");
  return result.value;
}

/** An api whose document holds these blocks (HTML as Rust renders it). */
async function apiWith(...html: string[]): Promise<Api> {
  const api = createMockApi();
  const base = value(await api.openDocument("docs/guide.md", []));
  const blocks: Block[] = html.map((h, at) => ({
    id: `b${at}`,
    kind: "p",
    lineStart: at,
    lineEnd: at,
    html: h,
  }));
  const view: DocumentView = {
    ...base,
    name: "guide.md",
    body: { kind: "markdown", blocks, outline: [], frontMatter: null },
  };
  return { ...api, openDocument: () => Promise.resolve({ ok: true, value: view }) };
}

const env = {
  assetUrl: (path: string) => `http://pm-asset.localhost/1/${encodeURIComponent(path)}`,
};

type Served = { bytes: number; type: string; status?: number };

/** `fetch` answering for the files named here (by the end of the URL); anything else is a 404. */
function serve(files: Record<string, Served>): string[] {
  const asked: string[] = [];
  vi.stubGlobal("fetch", (url: string) => {
    asked.push(url);
    const name = decodeURIComponent(url.split("/").pop() ?? url);
    const file = files[name] ?? files[url];
    if (!file) return Promise.resolve(new Response(null, { status: 404 }));
    const big = file.bytes > 1024;
    // a large file says its size and sends little: the builder must decide from the header
    const body = new Uint8Array(big ? 4 : file.bytes).fill(65);
    return Promise.resolve(
      new Response(body, {
        status: file.status ?? 200,
        headers: { "content-type": file.type, "content-length": String(file.bytes) },
      }),
    );
  });
  return asked;
}

const diagramEmbeds: EmbedLookup = (language) =>
  language === "mermaid" ? () => import("../../core/testing/DiagramProbe.svelte") : undefined;

async function build(api: Api, options: Partial<ExportOptions> = {}): Promise<string> {
  const outcome = await buildExport(api, "docs/guide.md", {
    target: "html",
    pageNumbers: false,
    env,
    embeds: () => undefined,
    ...options,
  });
  if (outcome.status !== "done") throw new Error(`export ${outcome.status}`);
  return outcome.html;
}

async function run(api: Api, options: Partial<ExportOptions> = {}): Promise<ExportOutcome> {
  return buildExport(api, "docs/guide.md", {
    target: "html",
    pageNumbers: false,
    env,
    embeds: () => undefined,
    ...options,
  });
}

beforeEach(() => serve({}));
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

describe("export page", () => {
  it("is one standalone page: doctype, language, title, CSP, no script, no handler", async () => {
    document.documentElement.lang = "pt-BR";
    const html = await build(
      await apiWith(
        '<h2 id="one">One</h2>',
        '<p onclick="steal()">text <a href="https://example.org" onmouseover="x()">web</a></p>',
        "<script>alert(1)</script>",
      ),
    ).finally(() => document.documentElement.removeAttribute("lang"));
    expect(html.startsWith("<!doctype html>")).toBe(true);
    expect(html).toContain('<html lang="pt-BR"');
    expect(html).toContain("<title>guide</title>");
    expect(html).toContain(
      `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; media-src data:; font-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">`,
    );
    expect(html).toContain('<main class="md md-body">');
    expect(html).not.toMatch(/<script/i);
    expect(html).not.toMatch(/\son[a-z]+=/i);
    // app-only marks and controls stay out
    expect(html).not.toContain("data-b=");
    expect(html).not.toContain('class="anchor"');
    expect(html).toContain('id="one"');
  });

  it("keeps web, mail and in-page links; a link to a project file keeps only its text", async () => {
    const html = await build(
      await apiWith(
        '<p><a href="https://example.org/a">web</a> <a href="mailto:a@b.org">mail</a> <a href="#one">here</a> <a href="notes/other.md">other note</a> <a href="javascript:alert(1)">bad</a></p>',
      ),
    );
    expect(html).toContain('href="https://example.org/a"');
    expect(html).toContain('href="mailto:a@b.org"');
    expect(html).toContain('href="#one"');
    expect(html).not.toContain("notes/other.md");
    expect(html).not.toContain("javascript:");
    expect(html).toContain("other note");
    expect(html).toContain("bad");
  });

  it("carries the document's pictures inside itself; an SVG file stays a picture", async () => {
    const asked = serve({
      "pics/a.png": { bytes: 8, type: "image/png" },
      "pics/d.svg": { bytes: 8, type: "image/svg+xml" },
    });
    const html = await build(
      await apiWith(
        '<p><img src="pm-asset:pics%2Fa.png" alt="A"></p>',
        '<p><img src="pm-asset:pics%2Fd.svg" alt="D"></p>',
      ),
    );
    expect(asked).toHaveLength(2);
    expect(html).toMatch(/<img[^>]+src="data:image\/png;base64,QUFBQUFBQUE="/);
    expect(html).toMatch(/<img[^>]+src="data:image\/svg\+xml;base64,/);
    expect(html).not.toContain("pm-asset");
    expect(html).not.toContain('loading="lazy"');
    expect(html).not.toMatch(/<svg/);
  });

  it("a picture that cannot be read leaves its description and a note", async () => {
    const outcome = await run(await apiWith('<p><img src="pm-asset:gone.png" alt="Lost"></p>'));
    if (outcome.status !== "done") throw new Error("not built");
    expect(outcome.html).not.toMatch(/<img/);
    expect(outcome.html).toContain("Lost");
    expect(outcome.notes).toEqual([{ kind: "picture", name: "gone.png" }]);
  });

  it("a picture over 25 MB is not carried: its description and a note", async () => {
    serve({ "huge.png": { bytes: 30 * 1024 * 1024, type: "image/png" } });
    const outcome = await run(await apiWith('<p><img src="pm-asset:huge.png" alt="Huge"></p>'));
    if (outcome.status !== "done") throw new Error("not built");
    expect(outcome.html).not.toMatch(/<img/);
    expect(outcome.html).toContain("Huge");
    expect(outcome.notes).toEqual([{ kind: "picture", name: "huge.png" }]);
  });

  it("hostile block HTML: no handler, script link, frame, form, base, stray asset or web fetch survives", async () => {
    const asked = serve({ "..%2Fx": { bytes: 3, type: "image/png" } });
    const outcome = await run(
      await apiWith(
        '<p><img src="pm-asset:gone.png" onerror="steal()" alt="Bad"></p>',
        '<p><a href="javascript:steal()">js link</a></p>',
        '<p><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><a xlink:href="javascript:steal()"><text>svg link</text></a></svg></p>',
        '<div><iframe src="https://example.org/frame"></iframe></div>',
        '<div><form action="https://example.org/post"><input name="q"></form></div>',
        '<div><base href="https://example.org/"><link rel="stylesheet" href="https://example.org/s.css"><meta http-equiv="refresh" content="0;url=https://example.org"></div>',
        '<div><object data="https://example.org/o"></object><embed src="https://example.org/e"></div>',
        '<p><img src="http://pm-asset.localhost/1/..%2Fx" alt="Stray"></p>',
        '<p><span class="remote-image" role="img" data-url="https://example.org/never.png" aria-label="Never">Never</span></p>',
      ),
    );
    if (outcome.status !== "done") throw new Error("not built");
    const { html } = outcome;
    const body = html.slice(html.indexOf("<body"));
    expect(html).not.toMatch(/\son[a-z]+=/i);
    expect(html).not.toContain("javascript:");
    expect(html).not.toMatch(/<(?:iframe|form|base|object|embed|link)\b/i);
    expect(body).not.toMatch(/<meta\b/i);
    expect(html).not.toContain("pm-asset.localhost");
    expect(html).not.toContain("example.org");
    expect(html).toContain("js link");
    expect(html).toContain("svg link");
    expect(html).toContain("Stray");
    expect(html).toContain("Never");
    // only the page's own asset URL was read; the stray one and the web picture never were
    expect(asked).toEqual(["http://pm-asset.localhost/1/gone.png"]);
  });

  it("a web picture: only one already fetched in this window comes in; the others stay a chip and a note", async () => {
    serve({ "blob:kept": { bytes: 3, type: "image/jpeg" } });
    vi.spyOn(webPictures, "kept").mockImplementation((url) =>
      url === "https://example.org/seen.jpg" ? "blob:kept" : undefined,
    );
    const outcome = await run(
      await apiWith(
        '<p><img src="https://example.org/seen.jpg" alt="Seen"> <img src="https://example.org/new.jpg" alt="New"></p>',
      ),
    );
    if (outcome.status !== "done") throw new Error("not built");
    expect(outcome.html).toMatch(/<img[^>]+src="data:image\/jpeg;base64,QUFB"[^>]*>/);
    expect(outcome.html).toContain("New");
    expect(outcome.html).not.toContain("https://example.org/new.jpg");
    expect(outcome.notes).toEqual([{ kind: "webPicture", name: "https://example.org/new.jpg" }]);
  });

  it("HTML: sound and video up to 25 MB come inside; a larger one leaves its still and a note", async () => {
    serve({
      "clip.mp4": { bytes: 10, type: "video/mp4" },
      "big.mp4": { bytes: 30 * 1024 * 1024, type: "video/mp4" },
      "big.png": { bytes: 2, type: "image/png" },
      "song.mp3": { bytes: 5, type: "audio/mpeg" },
    });
    const outcome = await run(
      await apiWith(
        '<p><video src="pm-asset:clip.mp4" controls></video></p>',
        '<p><video src="pm-asset:big.mp4" poster="pm-asset:big.png" controls></video></p>',
        '<p><audio controls><source src="pm-asset:song.mp3" type="audio/mpeg"></audio></p>',
      ),
    );
    if (outcome.status !== "done") throw new Error("not built");
    expect(outcome.html).toMatch(/<video[^>]+src="data:video\/mp4;base64,/);
    expect(outcome.html).toMatch(/<source[^>]+src="data:audio\/mpeg;base64,/);
    expect(outcome.html).toMatch(/<img[^>]+src="data:image\/png;base64,/);
    expect(outcome.html).toContain("Video left out: big.mp4");
    expect(outcome.html).not.toContain("pm-asset");
    expect(outcome.notes).toEqual([{ kind: "video", name: "big.mp4" }]);
  });

  it("paper: a video becomes its still, a sound a note", async () => {
    serve({
      "clip.mp4": { bytes: 10, type: "video/mp4" },
      "still.png": { bytes: 2, type: "image/png" },
      "song.mp3": { bytes: 5, type: "audio/mpeg" },
    });
    const outcome = await run(
      await apiWith(
        '<p><video src="pm-asset:clip.mp4" poster="pm-asset:still.png" controls></video></p>',
        '<p><audio src="pm-asset:song.mp3" controls></audio></p>',
        '<p><video src="pm-asset:clip.mp4" controls></video></p>',
      ),
      { target: "paper", page: A5 },
    );
    if (outcome.status !== "done") throw new Error("not built");
    expect(outcome.html).not.toMatch(/<video|<audio/);
    expect(outcome.html).toMatch(/<img[^>]+src="data:image\/png;base64,/);
    expect(outcome.html).toContain("Sound left out: song.mp3");
    expect(outcome.html).toContain("Video left out: clip.mp4");
    expect(outcome.notes).toEqual([
      { kind: "audio", name: "song.mp3" },
      { kind: "video", name: "clip.mp4" },
    ]);
  });

  it("paper: light look, the page rule from the page box, page numbers only when asked", async () => {
    const api = await apiWith("<p>text</p>");
    const numbered = await build(api, { target: "paper", page: A5, pageNumbers: true });
    expect(numbered).toContain('<meta name="color-scheme" content="light">');
    expect(numbered).toContain('data-theme="light"');
    expect(numbered).toContain("size: 148mm 210mm");
    expect(numbered).toContain("margin: 12.7mm");
    expect(numbered).toContain('counter(page) " / " counter(pages)');
    const plain = await build(api, { target: "paper", page: A5, pageNumbers: false });
    expect(plain).toContain("size: 148mm 210mm");
    expect(plain).not.toContain("counter(pages)");
    const web = await build(api, { target: "html" });
    expect(web).not.toContain("@page {");
    expect(web).toContain('<meta name="color-scheme" content="light dark">');
  });

  it("paper where the web view sets the margins itself: same margins, no margin boxes, same printable width", async () => {
    const api = await apiWith("<p>text</p>");
    let width = "";
    const observer = new MutationObserver(() => {
      const stage = document.querySelector<HTMLElement>(".pm-export-stage");
      if (stage) width = stage.style.width;
    });
    observer.observe(document.body, { childList: true });
    const html = await build(api, {
      target: "paper",
      page: A5,
      pageNumbers: true,
      nativeMargins: true,
    }).finally(() => observer.disconnect());
    expect(html).toContain("@page { size: 148mm 210mm; margin: 12.7mm; }");
    expect(html).not.toContain("@bottom-center");
    expect(width).toBe(`${148 - 2 * 12.7}mm`);
  });

  it("HTML: each diagram twice, light and dark, the reader's preference picks one", async () => {
    const html = await build(
      await apiWith('<pre><code class="language-mermaid">graph TD; A--&gt;B</code></pre>'),
      { embeds: diagramEmbeds },
    );
    expect(html).toMatch(/<figure[^>]+class="[^"]*pm-only-light[^"]*"[\s\S]*<desc>light<\/desc>/);
    expect(html).toMatch(/<figure[^>]+class="[^"]*pm-only-dark[^"]*"[\s\S]*<desc>dark<\/desc>/);
    expect(html).not.toContain("graph TD");
    expect(html).not.toContain("is-pending");
  });

  it("paper: diagrams in light only", async () => {
    const html = await build(
      await apiWith('<pre><code class="language-mermaid">graph TD; A--&gt;B</code></pre>'),
      { embeds: diagramEmbeds, target: "paper", page: A5 },
    );
    expect(html).toContain("<desc>light</desc>");
    expect(html).not.toContain("<desc>dark</desc>");
    expect(html).not.toContain("pm-only-dark");
  });

  it("a diagram that cannot be drawn shows a clear error, never its source", async () => {
    const html = await build(
      await apiWith('<pre><code class="language-mermaid">fail</code></pre>'),
      { embeds: diagramEmbeds, target: "paper", page: A5 },
    );
    expect(html).toContain("is-failed");
    expect(html).not.toMatch(/<pre[^>]*class="source"/);
    expect(html).not.toMatch(/>fail</);
  });

  it("code is copied once highlighted, without its buttons", async () => {
    const lookup: EmbedLookup = (language) =>
      language === "@code" ? () => import("../highlight/CodeBlock.svelte") : undefined;
    const html = await build(
      await apiWith('<pre><code class="language-js">const answer = 42;</code></pre>'),
      { embeds: lookup },
    );
    expect(html).toContain("hljs-keyword");
    expect(html).not.toMatch(/<button/);
    expect(html).not.toContain("aria-busy");
  });

  it("cancel: stops between steps and leaves nothing on the page", async () => {
    const controller = new AbortController();
    const api = await apiWith(
      '<pre><code class="language-mermaid">graph TD; A--&gt;B</code></pre>',
    );
    const outcome = await run(
      {
        ...api,
        openDocument: async (path, known) => {
          const result = await api.openDocument(path, known);
          controller.abort();
          return result;
        },
      },
      { embeds: diagramEmbeds, signal: controller.signal },
    );
    expect(outcome.status).toBe("cancelled");
    expect(document.body.children).toHaveLength(0);
  });

  /** An embed whose code never arrives (a hung import). */
  const hungEmbeds: EmbedLookup = (language) =>
    language === "mermaid" ? () => new Promise(() => {}) : undefined;
  const hungDiagram = '<pre><code class="language-mermaid">graph TD; A--&gt;B</code></pre>';

  it("cancel while an embed is still loading: the run ends and the copy of the page goes too", async () => {
    const controller = new AbortController();
    const outcome = run(await apiWith(hungDiagram), {
      embeds: hungEmbeds,
      signal: controller.signal,
    });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(document.body.children.length).toBeGreaterThan(0);
    controller.abort();
    expect((await outcome).status).toBe("cancelled");
    expect(document.body.children).toHaveLength(0);
  });

  it("an embed that never loads cannot hold the build past its deadline", async () => {
    const outcome = await run(await apiWith(hungDiagram), {
      embeds: hungEmbeds,
      target: "paper",
      page: A5,
      settleMs: 50,
    });
    expect(outcome.status).toBe("done");
    expect(document.body.children).toHaveLength(0);
  });

  /** `decode` as a web view does it: a picture marked lazy that is not on screen never settles. */
  function decodeLike(answer: (img: HTMLImageElement) => Promise<void>): () => void {
    Object.defineProperty(HTMLImageElement.prototype, "decode", {
      configurable: true,
      value(this: HTMLImageElement) {
        return answer(this);
      },
    });
    return () => {
      Reflect.deleteProperty(HTMLImageElement.prototype, "decode");
    };
  }
  const never = (): Promise<void> => new Promise(() => {});

  it("a picture the reading view marks lazy is still read and carried", async () => {
    serve({ "pics/a.png": { bytes: 8, type: "image/png" } });
    const restore = decodeLike((img) =>
      img.getAttribute("loading") === "lazy" ? never() : Promise.resolve(),
    );
    const outcome = await run(
      await apiWith('<p><img src="pm-asset:pics%2Fa.png" alt="A"></p>'),
    ).finally(restore);
    if (outcome.status !== "done") throw new Error("not built");
    expect(outcome.html).toMatch(/<img[^>]+src="data:image\/png;base64,/);
    expect(outcome.html).not.toContain("loading=");
    expect(outcome.notes).toEqual([]);
  });

  it("cancel while a picture is decoding ends the run", async () => {
    serve({ "pics/a.png": { bytes: 8, type: "image/png" } });
    const controller = new AbortController();
    const restore = decodeLike(() => {
      setTimeout(() => controller.abort(), 0);
      return never();
    });
    const outcome = await run(await apiWith('<p><img src="pm-asset:pics%2Fa.png" alt="A"></p>'), {
      signal: controller.signal,
    }).finally(restore);
    expect(outcome.status).toBe("cancelled");
    expect(document.body.children).toHaveLength(0);
  });

  it("a picture that never decodes stops holding the build: its description and a note", async () => {
    serve({ "pics/a.png": { bytes: 8, type: "image/png" } });
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let decoding = false;
    const restore = decodeLike(() => {
      decoding = true;
      return never();
    });
    try {
      const outcome = run(await apiWith('<p><img src="pm-asset:pics%2Fa.png" alt="A"></p>'));
      await vi.waitFor(() => expect(decoding).toBe(true));
      await vi.advanceTimersByTimeAsync(10_000);
      const ended = await outcome;
      if (ended.status !== "done") throw new Error("not built");
      expect(ended.html).not.toMatch(/<img/);
      expect(ended.notes).toEqual([{ kind: "picture", name: "a.png" }]);
    } finally {
      restore();
      vi.useRealTimers();
    }
  });

  it("a document that cannot be read is a failure with its error", async () => {
    const api = createMockApi();
    const outcome = await buildExport(api, "nowhere/missing.md", {
      target: "html",
      pageNumbers: false,
      env,
    });
    expect(outcome.status).toBe("failed");
  });
});
