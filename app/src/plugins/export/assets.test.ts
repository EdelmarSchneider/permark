import { afterEach, describe, expect, it, vi } from "vitest";
import { dataUri, fetchDataUri, inlineFontUrls, MEDIA_MAX_BYTES } from "./assets";

afterEach(() => vi.unstubAllGlobals());

function answer(bytes: number, headers: Record<string, string>, status = 200): void {
  vi.stubGlobal("fetch", () =>
    Promise.resolve(new Response(new Uint8Array(bytes).fill(66), { status, headers })),
  );
}

describe("data URIs", () => {
  it("bytes become base64 with their type", () => {
    expect(dataUri(new Uint8Array([104, 105]), "text/plain")).toBe("data:text/plain;base64,aGk=");
  });

  it("a fetched file keeps the type it was served with; an SVG without one is still an SVG", async () => {
    answer(2, { "content-type": "image/png" });
    expect(await fetchDataUri("http://pm-asset.localhost/1/a.png")).toEqual({
      status: "ok",
      uri: "data:image/png;base64,QkI=",
    });
    answer(2, {});
    expect(await fetchDataUri("http://pm-asset.localhost/1/d.svg")).toEqual({
      status: "ok",
      uri: "data:image/svg+xml;base64,QkI=",
    });
  });

  it("a file over the limit is not read: its declared size decides, else its bytes", async () => {
    answer(1, { "content-type": "video/mp4", "content-length": "100" });
    expect(await fetchDataUri("http://x/v.mp4", { max: 10 })).toEqual({ status: "tooLarge" });
    answer(20, { "content-type": "video/mp4" });
    expect(await fetchDataUri("http://x/v.mp4", { max: 10 })).toEqual({ status: "tooLarge" });
  });

  it("without a declared size, reading stops once the limit is passed", async () => {
    let pulled = 0;
    const endless = new ReadableStream<Uint8Array>({
      pull(controller) {
        pulled += 1;
        controller.enqueue(new Uint8Array(4).fill(66));
      },
    });
    vi.stubGlobal("fetch", () => Promise.resolve(new Response(endless)));
    expect(await fetchDataUri("http://x/v.mp4", { max: 10 })).toEqual({ status: "tooLarge" });
    expect(pulled).toBeLessThan(10);
  });

  it("pictures are capped too: the default limit is the media one", async () => {
    answer(1, { "content-type": "image/png", "content-length": String(MEDIA_MAX_BYTES + 1) });
    expect(await fetchDataUri("http://x/a.png")).toEqual({ status: "tooLarge" });
  });

  it("an error answer or a refused request is a failure; a data URI is kept as is", async () => {
    answer(0, {}, 404);
    expect(await fetchDataUri("http://x/a.png")).toEqual({ status: "failed" });
    vi.stubGlobal("fetch", () => Promise.reject(new TypeError("refused")));
    expect(await fetchDataUri("http://x/a.png")).toEqual({ status: "failed" });
    expect(await fetchDataUri("data:image/png;base64,AA==")).toEqual({
      status: "ok",
      uri: "data:image/png;base64,AA==",
    });
  });
});

describe("font URLs", () => {
  const files = new Map([
    ["KaTeX_Main-Bold", "data:font/woff2;base64,Qk9MRA=="],
    ["KaTeX_Main-BoldItalic", "data:font/woff2;base64,Qkk="],
    ["inter-latin-wght-normal", "data:font/woff2;base64,SU4="],
  ]);

  it("source paths and hashed built names both find their file, never a longer name's", () => {
    const css = [
      'src:url(fonts/KaTeX_Main-Bold.woff2) format("woff2")',
      'src:url(/assets/KaTeX_Main-BoldItalic-Ab-9xY_z.woff2) format("woff2")',
      'src: url("../../../node_modules/@fontsource-variable/inter/files/inter-latin-wght-normal.woff2")',
      'src: url("/assets/inter-latin-ext-wght-normal-Zz12.woff2")',
    ].join("\n");
    const out = inlineFontUrls(css, files);
    expect(out).toContain('url("data:font/woff2;base64,Qk9MRA==") format("woff2")');
    expect(out).toContain('url("data:font/woff2;base64,Qkk=") format("woff2")');
    expect(out).toContain('url("data:font/woff2;base64,SU4=")');
    // a font not listed keeps its URL
    expect(out).toContain('url("/assets/inter-latin-ext-wght-normal-Zz12.woff2")');
  });
});
