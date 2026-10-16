// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { enhanceBlocks, stashRelativeImages, type EnhanceContext } from "./enhance";

/** The files of the folder the document asked for, in order. */
let asked: string[] = [];

function context(over: Partial<EnhanceContext> = {}): EnhanceContext {
  return {
    docPath: "docs/guide.md",
    theme: "light",
    labels: {
      copyCode: "Copy",
      anchorLabel: (name) => name,
      diagramPending: "...",
      diagramFailed: "failed",
      alertTitles: {},
      codeAuto: (name) => `auto: ${name}`,
      codeAutoHint: "Guessed language",
    },
    resolveImage: async (path) => {
      asked.push(path);
      return `tm://${path}`;
    },
    ...over,
  };
}

function fragment(html: string): HTMLElement {
  const template = document.createElement("template");
  template.innerHTML = html;
  stashRelativeImages(template.content, "docs/guide.md");
  const host = document.createElement("div");
  host.append(template.content);
  return host;
}

beforeEach(() => {
  asked = [];
  // jsdom plays nothing
  HTMLMediaElement.prototype.load = vi.fn();
});

describe("stashRelativeImages", () => {
  it("holds back the files of players and posters, like those of pictures, until they can be resolved", () => {
    const host = fragment(
      '<img src="../img/a.png"><video src="clips/a.mp4" poster="clips/a.png" controls></video>' +
        '<audio controls><source src="snd/b.ogg" type="audio/ogg"></audio>',
    );
    expect(host.querySelector("img")?.hasAttribute("src")).toBe(false);
    expect(host.querySelector("img")?.getAttribute("data-tm-path")).toBe("img/a.png");
    const video = host.querySelector("video");
    expect(video?.hasAttribute("src")).toBe(false);
    expect(video?.getAttribute("data-tm-path")).toBe("docs/clips/a.mp4");
    expect(video?.hasAttribute("poster")).toBe(false);
    expect(video?.getAttribute("data-tm-poster")).toBe("docs/clips/a.png");
    const source = host.querySelector("source");
    expect(source?.hasAttribute("src")).toBe(false);
    expect(source?.getAttribute("data-tm-path")).toBe("docs/snd/b.ogg");
    expect(source?.getAttribute("type")).toBe("audio/ogg");
  });

  it("leaves a web address alone", () => {
    const host = fragment('<video src="https://example.com/a.mp4" controls></video>');
    expect(host.querySelector("video")?.getAttribute("src")).toBe("https://example.com/a.mp4");
    expect(host.querySelector("video")?.hasAttribute("data-tm-path")).toBe(false);
  });
});

describe("a player of the document", () => {
  it("is pointed at its files once it is in the page", async () => {
    const host = fragment(
      '<video src="clips/a.mp4" poster="clips/a.png" controls></video>' +
        '<audio controls><source src="snd/b.ogg"><source src="snd/b.mp3"></audio>',
    );
    const blocks = Array.from(host.children) as HTMLElement[];
    const { tasks } = enhanceBlocks(blocks, context());
    await Promise.all(tasks);

    const video = host.querySelector("video");
    expect(video?.getAttribute("src")).toBe("tm://docs/clips/a.mp4");
    expect(video?.getAttribute("poster")).toBe("tm://docs/clips/a.png");
    const sources = Array.from(host.querySelectorAll("source"));
    expect(sources.map((s) => s.getAttribute("src"))).toEqual([
      "tm://docs/snd/b.ogg",
      "tm://docs/snd/b.mp3",
    ]);
    // the player was drawn before its sources had addresses: it loads again to read them
    expect(HTMLMediaElement.prototype.load).toHaveBeenCalledTimes(2);
  });

  it("asks for a file once however many players name it", async () => {
    // (the addresses already asked for are kept across the page, so this file is one no other test uses)
    const host = fragment(
      '<video src="clips/once.mp4" controls></video><video src="clips/once.mp4" controls></video>',
    );
    const { tasks } = enhanceBlocks(Array.from(host.children) as HTMLElement[], context());
    await Promise.all(tasks);
    expect(asked.filter((p) => p === "docs/clips/once.mp4")).toHaveLength(1);
  });

  it("is marked broken when its file cannot be resolved", async () => {
    const host = fragment('<video src="clips/gone.mp4" controls></video>');
    const { tasks } = enhanceBlocks(
      Array.from(host.children) as HTMLElement[],
      context({
        resolveImage: async () => {
          throw new Error("no such file");
        },
      }),
    );
    await Promise.all(tasks);
    expect(host.querySelector("video")?.classList.contains("is-broken")).toBe(true);
  });

  it("leaves a player with a web address to the webview", async () => {
    const host = fragment('<video src="https://example.com/a.mp4" controls></video>');
    const { tasks } = enhanceBlocks(Array.from(host.children) as HTMLElement[], context());
    await Promise.all(tasks);
    expect(asked).toEqual([]);
    expect(HTMLMediaElement.prototype.load).not.toHaveBeenCalled();
  });
});
