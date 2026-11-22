// @vitest-environment jsdom
// An agent's message: plain text while it comes; once whole, the plugin's sanitized blocks drawn as a document's
// (raw markup of the message never becomes elements, a failed render keeps the text, a late answer is dropped); web
// links open in the browser, project links | code references open the file at the line (`#L3`) | heading, a link that
// leads nowhere is said, a web picture stays an inert chip, a link inside the message stays in it; its headings sit
// below the panel's; only the last message keeps the plain text's height. A replayed message never shows its raw
// markup before the blocks (its plain text only when the render fails).
import "../../core/testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../../api";
import { t } from "../../core/i18n";
import { Box } from "../../core/testing/box.svelte";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { MOCK_FOLDER } = await import("../../api/mock");
const { acp } = await import("./store.svelte");
const { documents } = await import("../../core/doc/document.svelte");
const { windowStore } = await import("../../core/window.svelte");
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

function show(text: Box<string>, final: Box<boolean>, last = new Box(false), quiet = false): void {
  component = mount(MarkdownReply, {
    target: root,
    props: {
      get text() {
        return text.value;
      },
      get final() {
        return final.value;
      },
      get last() {
        return last.value;
      },
      quiet,
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
    // below the panel's own headings, the level written kept for the look
    expect(body().querySelector("h2")).toBeNull();
    expect(body().querySelector("h5")?.textContent).toBe("Done");
    expect(body().querySelector("h5")?.getAttribute("data-level")).toBe("2");
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

  it("replayed (quiet): never shows its raw markup before the blocks come", async () => {
    let answer: (value: readonly Block[] | null) => void = () => {};
    const render = vi.spyOn(acp, "render").mockImplementation(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );
    const final = new Box(false);
    show(new Box("**x**"), final, new Box(true), true);
    await settle();
    // the last replayed line before the agent is ready: a placeholder, nothing asked yet
    expect(root.querySelector(".text")).toBeNull();
    expect(root.textContent).not.toContain("**x**");
    expect(root.querySelector('[aria-busy="true"]')).not.toBeNull();
    expect(render).not.toHaveBeenCalled();
    final.value = true;
    flushSync();
    await settle();
    expect(render).toHaveBeenCalledTimes(1);
    expect(root.querySelector(".text")).toBeNull();
    expect(root.textContent).not.toContain("**x**");
    answer(blocks('<p data-b="b0"><strong>x</strong></p>'));
    await settle();
    expect(body().querySelector("strong")?.textContent).toBe("x");
    expect(root.querySelector('[aria-busy="true"]')).toBeNull();
  });

  it("replayed (quiet): keeps its plain text when it cannot be rendered", async () => {
    vi.spyOn(acp, "render").mockResolvedValue(null);
    show(new Box("**x**"), new Box(true), new Box(false), true);
    await settle();
    expect(root.querySelector(".text")?.textContent).toBe("**x**");
    expect(root.querySelector('[aria-busy="true"]')).toBeNull();
    if (component) unmount(component);
    vi.spyOn(acp, "render").mockRejectedValue(new Error("gone"));
    show(new Box("**y**"), new Box(true), new Box(false), true);
    await settle();
    expect(root.querySelector(".text")?.textContent).toBe("**y**");
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

  it("opens a file link at the line its anchor names, else at its heading; a link that leads nowhere is said", async () => {
    vi.spyOn(acp, "render").mockResolvedValue(
      blocks(
        '<p data-b="b0"><a href="docs/guide.md#L3">line</a> <a href="docs/guide.md#setup">part</a> <a href="nope.md">gone</a> <a href="../up.md">up</a></p>',
      ),
    );
    const openAt = vi.spyOn(acp, "openAt").mockImplementation(() => {});
    const navigate = vi.spyOn(documents, "navigate").mockImplementation(() => {});
    const notify = vi.spyOn(windowStore, "notifyKeyed");
    show(new Box("x"), new Box(true));
    await settle();
    const link = (name: string): Element => {
      const found = [...body().querySelectorAll("a")].find((a) => a.textContent === name);
      if (!found) throw new Error(`no link ${name}`);
      return found;
    };
    click(link("line"));
    await settle();
    expect(openAt).toHaveBeenCalledWith("docs/guide.md", 3);
    click(link("part"));
    await settle();
    expect(navigate).toHaveBeenCalledWith("docs/guide.md", "setup");
    click(link("gone"));
    await settle();
    expect(notify).toHaveBeenLastCalledWith("acp.link", t("link.missing", { path: "nope.md" }));
    click(link("up"));
    await settle();
    expect(notify).toHaveBeenCalledTimes(2);
    expect(openAt).toHaveBeenCalledTimes(1);
  });

  it("keeps the plain text's height only while it is the conversation's last line", async () => {
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(252);
    vi.spyOn(acp, "render").mockResolvedValue(blocks('<p data-b="b0">short</p>'));
    const last = new Box(true);
    show(new Box("a long plain text"), new Box(true), last);
    await settle();
    expect(body().style.minHeight).toBe("252px");
    last.value = false;
    flushSync();
    expect(body().style.minHeight).toBe("");
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
