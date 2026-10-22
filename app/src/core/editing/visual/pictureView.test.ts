// @vitest-environment jsdom
import "./jsdomShims";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { Selection } from "@milkdown/kit/prose/state";
import type { Segment } from "../../../api/generated/Segment";
import { pictureSource } from "../pictures";
import { run } from "./commands";
import { buildDoc, compose } from "./compose";
import { createCore, type Core, type PictureView } from "./core";

const DOC = "docs/guide.md";
let core: Core;
let host: HTMLElement;

/** The app's host: files through a fake asset URL, web pictures as chips. */
function view(src: string, alt: string): PictureView {
  const source = pictureSource(DOC, src);
  switch (source.kind) {
    case "file":
      return { kind: "load", url: `asset://localhost/${source.path}` };
    case "inline":
      return { kind: "load", url: source.src };
    case "web":
      return { kind: "chip", text: alt || "web picture", title: source.url };
    case "none":
      return { kind: "none" };
  }
}

beforeAll(async () => {
  host = document.createElement("div");
  document.body.append(host);
  core = await createCore({ host, pictures: { view } });
});

afterAll(async () => {
  await core.destroy();
});

function segmentsOf(text: string): Segment[] {
  const lines = text.split(/\r?\n/);
  const out: Segment[] = [];
  let start = -1;
  lines.forEach((line, i) => {
    const filled = line.trim() !== "";
    if (filled && start < 0) start = i;
    if (!filled && start >= 0) {
      out.push({ lineStart: start + 1, lineEnd: i, kind: "paragraph", lock: null });
      start = -1;
    }
  });
  if (start >= 0)
    out.push({ lineStart: start + 1, lineEnd: lines.length, kind: "paragraph", lock: null });
  return out;
}

async function load(text: string) {
  const built = await buildDoc(core, text, segmentsOf(text));
  core.setDoc(built.doc);
  return built;
}

describe("Edit draws the document's pictures like View", () => {
  it("loads a picture next to the document through the asset URL; the text stays byte for byte", async () => {
    const text = "Intro\r\n\r\n![logo](img/logo.png  'The logo')\r\n\r\n![](../a%20b.png)\r\n";
    const built = await load(text);
    const imgs = [...host.querySelectorAll<HTMLImageElement>("img:not(.ProseMirror-separator)")];
    expect(imgs.map((img) => img.getAttribute("src"))).toEqual([
      "asset://localhost/docs/img/logo.png",
      "asset://localhost/a b.png",
    ]);
    expect(imgs[0]?.alt).toBe("logo");
    expect(imgs[0]?.title).toBe("The logo");
    const out = compose(core, built, core.view.state.doc);
    expect(out.text).toBe(text);
    expect(out.changed).toBe(0);
  });

  it("a web picture is a chip (never fetched); above the root draws no source", async () => {
    await load("![chart](https://example.com/c.png)\n\n![x](../../../up.png)\n");
    const chip = host.querySelector(".remote-image");
    expect(chip?.textContent).toBe("chart");
    expect(chip?.getAttribute("title")).toBe("https://example.com/c.png");
    expect(host.querySelector("img[src*='example.com']")).toBeNull();
    const missing = host.querySelector("img.pm-picture-missing");
    expect(missing?.hasAttribute("src")).toBe(false);
    expect(missing?.getAttribute("alt")).toBe("x");
  });

  it("an edit elsewhere leaves the picture's line as written", async () => {
    const text = "first\n\n![logo](img/logo.png)\n";
    const built = await load(text);
    const view = core.view;
    view.dispatch(view.state.tr.insertText("the ", 1));
    const out = compose(core, built, view.state.doc);
    expect(out.text).toBe("the first\n\n![logo](img/logo.png)\n");
  });

  it("a picture inserted in Edit keeps its written source and draws at once", async () => {
    await load("text\n");
    const view = core.view;
    view.dispatch(view.state.tr.setSelection(Selection.atEnd(view.state.doc)));
    expect(run(core, { kind: "image", src: "img/new%20one.png", alt: "new" })).toBe(true);
    expect(host.querySelector("img:not(.ProseMirror-separator)")?.getAttribute("src")).toBe(
      "asset://localhost/docs/img/new one.png",
    );
    expect(core.serialize(view.state.doc)).toContain("![new](img/new%20one.png)");
  });
});
