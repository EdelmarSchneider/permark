// @vitest-environment jsdom
// An agent's message: plain text while it comes; once whole, the plugin's sanitized blocks drawn as a document's
// (raw markup of the message never becomes elements, a failed render keeps the text, a late answer is dropped); web
// links open in the browser, project links | code references open the file at the line, a web picture stays an inert
// chip, a link inside the message stays in it.
import "../../core/testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../../api";
import { Box } from "../../core/testing/box.svelte";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { MOCK_FOLDER } = await import("../../api/mock");
const { acp } = await import("./store.svelte");
const MarkdownReply = (await import("./MarkdownReply.svelte")).default;

let component: Record<string, unknown> | null = null;
let root: HTMLElement;

/** Blocks as the plugin renders them (one element each). */
function blocks(...html: string[]): Block[] {
  return html.map((h, at) => ({
    id: `b${String(at)}`,
    kind: "p",
    lineStart: at + 1,
    lineEnd: at + 1,
    html: h,
  }));
}

function show(text: Box<string>, final: Box<boolean>): void {
  component = mount(MarkdownReply, {
    target: root,
    props: {
      get text() {
        return text.value;
      },
      get final() {
        return final.value;
      },
    },
  });
  flushSync();
}

async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
}

/** A click as the person makes it; true when the web view's own way was kept from it. */
function click(element: Element): boolean {
  const event = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 });
  element.dispatchEvent(event);
  return event.defaultPrevented;
}

function body(): HTMLElement {
  const found = root.querySelector<HTMLElement>(".md.md-body");
  if (found === null) throw new Error("not rendered");
  return found;
}

beforeEach(async () => {
  await api.openPath(MOCK_FOLDER, "thisTab");
  root = document.createElement("div");
  document.body.append(root);
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  root.remove();
  vi.restoreAllMocks();
});

describe("an agent's message", () => {
  it("is plain text while it comes: nothing is rendered", async () => {
    const render = vi.spyOn(acp, "render");
    show(new Box("**half"), new Box(false));
    await settle();
    expect(root.querySelector(".text")?.textContent).toBe("**half");
    expect(root.querySelector(".md-body")).toBeNull();
    expect(render).not.toHaveBeenCalled();
  });

  it("once whole, shows the plugin's sanitized blocks, rendered once; its own markup never becomes elements", async () => {
    const text = "## Done\n\n**bold** <img src=x onerror=alert(1)>";
    const render = vi
      .spyOn(acp, "render")
      .mockResolvedValue(
        blocks(
          '<h2 data-b="b0" id="done">Done</h2>',
          '<p data-b="b1"><strong>bold</strong> &lt;img src=x onerror=alert(1)&gt;</p>',
        ),
      );
    const final = new Box(false);
    show(new Box(text), final);
    final.value = true;
    flushSync();
    await settle();
    expect(render).toHaveBeenCalledTimes(1);
    expect(render).toHaveBeenCalledWith(text);
    expect(body().querySelector("h2")?.textContent).toBe("Done");
    expect(body().querySelector("strong")?.textContent).toBe("bold");
    expect(body().querySelector("img")).toBeNull();
    expect(root.querySelector(".text")).toBeNull();
    // no heading anchor, no id that could answer for the open document's own headings
    expect(body().querySelector("a.anchor, [id]")).toBeNull();
    flushSync();
    await settle();
    expect(render).toHaveBeenCalledTimes(1);
  });

  it("keeps its plain text when it cannot be rendered", async () => {
    vi.spyOn(acp, "render").mockResolvedValue(null);
    show(new Box("# Title"), new Box(true));
    await settle();
    expect(root.querySelector(".text")?.textContent).toBe("# Title");
    expect(root.querySelector(".md-body")).toBeNull();
  });

  it("drops an answer for text it no longer shows", async () => {
    const answers: ((value: readonly Block[] | null) => void)[] = [];
    vi.spyOn(acp, "render").mockImplementation(
      () => new Promise((resolve) => answers.push(resolve)),
    );
    const text = new Box("old");
    show(text, new Box(true));
    text.value = "new";
    flushSync();
    expect(answers).toHaveLength(2);
    answers[1]?.(blocks('<p data-b="b0">new</p>'));
    await settle();
    answers[0]?.(blocks('<p data-b="b0">old</p>'));
    await settle();
    expect(body().textContent).toBe("new");
  });

  it("opens web links in the browser and project links | code references at their file; the page never navigates", async () => {
    vi.spyOn(acp, "render").mockResolvedValue(
      blocks(
        '<p data-b="b0"><a href="https://commonmark.org">web</a> <a href="mailto:a@b.c">mail</a> <a href="docs/guide.md">guide</a> <a href="ftp://x">ftp</a></p>',
        '<p data-b="b1">See <code>docs/guide.md:3</code></p>',
      ),
    );
    const website = vi.spyOn(acp, "openWebsite").mockImplementation(() => {});
    const openAt = vi.spyOn(acp, "openAt").mockImplementation(() => {});
    show(new Box("x"), new Box(true));
    await settle();
    const link = (name: string): Element => {
      const found = [...body().querySelectorAll("a")].find((a) => a.textContent === name);
      if (!found) throw new Error(`no link ${name}`);
      return found;
    };
    expect(click(link("web"))).toBe(true);
    expect(click(link("mail"))).toBe(true);
    expect(website.mock.calls).toEqual([["https://commonmark.org"], ["mailto:a@b.c"]]);
    expect(click(link("ftp"))).toBe(true);
    expect(click(link("guide"))).toBe(true);
    await settle();
    expect(openAt).toHaveBeenCalledWith("docs/guide.md", null);
    expect(website).toHaveBeenCalledTimes(2);

    const ref = body().querySelector<HTMLElement>(".code-ref");
    if (ref === null) throw new Error("no code reference");
    expect(ref.tabIndex).toBe(0);
    expect(click(ref)).toBe(true);
    expect(openAt).toHaveBeenLastCalledWith("docs/guide.md", 3);
    openAt.mockClear();
    ref.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(openAt).toHaveBeenCalledWith("docs/guide.md", 3);
  });

  it("leaves a web picture an inert chip and keeps a link inside the message in it", async () => {
    const scroll = vi.fn();
    Element.prototype.scrollIntoView = scroll;
    vi.spyOn(acp, "render").mockResolvedValue(
      blocks(
        '<p data-b="b0"><img src="https://example.com/map.png" alt="Map"> <a href="#fn-1">1</a></p>',
        '<section data-b="b1" class="footnotes"><ol><li id="fn-1">Note</li></ol></section>',
      ),
    );
    const website = vi.spyOn(acp, "openWebsite").mockImplementation(() => {});
    const openAt = vi.spyOn(acp, "openAt").mockImplementation(() => {});
    show(new Box("x"), new Box(true));
    await settle();
    const chip = body().querySelector(".remote-image");
    if (chip === null) throw new Error("no chip");
    expect(chip.textContent).toBe("Map");
    expect(chip.hasAttribute("data-url")).toBe(false);
    expect(chip.getAttribute("title")).toBe("https://example.com/map.png");
    click(chip);
    const note = body().querySelector("a");
    if (note === null) throw new Error("no link");
    expect(click(note)).toBe(true);
    expect(scroll).toHaveBeenCalledTimes(1);
    expect(website).not.toHaveBeenCalled();
    expect(openAt).not.toHaveBeenCalled();
  });
});
