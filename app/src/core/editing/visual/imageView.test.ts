// @vitest-environment jsdom
// A local picture paints from the asset address as soon as the node exists. The markdown path stays on the node.
import "./jsdomShims";
import { afterEach, describe, expect, it } from "vitest";
import { pictureDisplayUrl } from "../pictures";
import { createCore, type Core } from "./core";
import { appPlugins, type PluginHost } from "./plugins";

const DOC = "docs/note.md";

function display(src: string): string {
  return pictureDisplayUrl(DOC, src, (path) => `pm://${path}`);
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
});
