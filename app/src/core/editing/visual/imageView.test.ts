// @vitest-environment jsdom
// A local picture paints from the asset address as soon as the node exists. The markdown path stays on the node.
import "./jsdomShims";
import { afterEach, describe, expect, it } from "vitest";
import { pictureDisplayUrl } from "../pictures";
import { createCore, type Core } from "./core";
import { appPlugins, type PluginHost } from "./plugins";

const DOC = "docs/note.md";
/** Web pictures the person already fetched in this window. */
const kept = new Map<string, string>([["https://example.com/kept.png", "blob:kept-1"]]);

function display(src: string): string | null {
  return pictureDisplayUrl(
    DOC,
    src,
    (path) => `pm://${path}`,
    (url) => kept.get(url),
  );
}

function host(): PluginHost {
  return {
    core: () => null,
    changed: () => {},
    selected: () => {},
    save: () => {},
    modeKey: () => {},
    linkKey: () => {},
    slash: () => {},
    slashKey: () => false,
    pictures: () => {},
    followLink: () => {},
    displaySrc: display,
  };
}

describe("edit image view", () => {
  let core: Core | null = null;

  afterEach(async () => {
    await core?.destroy();
    core = null;
    document.body.replaceChildren();
  });

  it("paints a picture already in the file, and one just inserted, without writing the asset address", async () => {
    const el = document.createElement("div");
    document.body.append(el);
    core = await createCore({ host: el, first: appPlugins(host()) });
    core.setDoc(core.parse("Before ![logo](img/a.png) after.\n"));
    const loaded = el.querySelector("img");
    expect(loaded?.getAttribute("src")).toBe("pm://docs/img/a.png");
    expect(loaded?.getAttribute("alt")).toBe("logo");

    const image = core.view.state.schema.nodes.image;
    if (!image) throw new Error("no image node");
    const node = image.create({ src: "shot.png", alt: "", title: "" });
    core.view.dispatch(core.view.state.tr.insert(1, node).scrollIntoView());
    const srcs = [...el.querySelectorAll("img")].map((img) => img.getAttribute("src"));
    expect(srcs).toContain("pm://docs/shot.png");

    const written = core.serialize(core.view.state.doc);
    expect(written).toContain("img/a.png");
    expect(written).toContain("shot.png");
    expect(written).not.toContain("pm://");
  });

  it("shows View's chip for a picture on the web (nothing is fetched); one fetched in this window paints", async () => {
    const el = document.createElement("div");
    document.body.append(el);
    core = await createCore({ host: el, first: appPlugins(host()) });
    core.setDoc(core.parse("![far](https://example.com/a.png) and ![](//example.com/b.png)\n"));
    const pictures = (): HTMLImageElement[] => [
      ...el.querySelectorAll<HTMLImageElement>("img:not(.ProseMirror-separator)"),
    ];
    expect(pictures()).toHaveLength(0);
    const chips = [...el.querySelectorAll(".remote-image")];
    expect(chips.map((chip) => chip.textContent)).toEqual(["far", "Picture from the web"]);
    expect(chips[0]?.getAttribute("aria-label")).toBe("far");
    expect(chips[0]?.getAttribute("title")).toBe("https://example.com/a.png");
    // no fetch of View's swaps this chip: the editor owns its place
    expect(chips[0]?.hasAttribute("data-url")).toBe(false);

    // the address edited to a project file: the picture paints; back to the web: the chip again
    const at = (): number => {
      let found = -1;
      core?.view.state.doc.descendants((node, pos) => {
        if (found < 0 && node.type.name === "image") found = pos;
      });
      return found;
    };
    const set = (src: string): void => {
      if (!core) return;
      const pos = at();
      const node = core.view.state.doc.nodeAt(pos);
      core.view.dispatch(core.view.state.tr.setNodeMarkup(pos, undefined, { ...node?.attrs, src }));
    };
    set("img/a.png");
    expect(pictures().map((img) => img.getAttribute("src"))).toEqual(["pm://docs/img/a.png"]);
    set("https://example.com/kept.png");
    expect(pictures().map((img) => img.getAttribute("src"))).toEqual(["blob:kept-1"]);
    set("https://example.com/c.png");
    expect(pictures()).toHaveLength(0);
    expect(el.querySelectorAll(".remote-image")).toHaveLength(2);
    expect(core.serialize(core.view.state.doc)).toContain("https://example.com/c.png");
  });
});
