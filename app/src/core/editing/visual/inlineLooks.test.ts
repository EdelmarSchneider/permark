// @vitest-environment jsdom
import "./jsdomShims";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { undo } from "@milkdown/kit/prose/history";
import { TextSelection } from "@milkdown/kit/prose/state";
import { $prose } from "@milkdown/kit/utils";
import type { LinkTarget } from "../../../api";
import { findTextRefs } from "../../doc/codeRefs";
import type { CodeRefTarget } from "../../doc/decorate";
import { buildDoc, compose } from "./compose";
import { createCore, type Core } from "./core";
import { LOOKS_DELAY_MS, repaintLooks, type LooksHost } from "./inlineLooks";
import { marksPlugin, setMarks } from "./marks";

// counts the scans of plain text: a block that did not change is not read again
vi.mock("../../doc/codeRefs", async (load) => {
  const real = await load<typeof import("../../doc/codeRefs")>();
  return { ...real, findTextRefs: vi.fn(real.findTextRefs) };
});

let core: Core;
const files = new Set<string>();
let prompt = false;
let askedLinks: string[] = [];
let askedRefs: string[] = [];
let forgets = 0;
let opened: CodeRefTarget[] = [];
let filesChanged: (() => void) | null = null;

const host: LooksHost = {
  links: (hrefs) => {
    askedLinks.push(...hrefs);
    return Promise.resolve(
      hrefs.map((href): LinkTarget => {
        const path = href.replace(/^\//, "");
        return files.has(path) ? { type: "doc", path, anchor: null } : { type: "missing", path };
      }),
    );
  },
  codeRefs: (written) => {
    askedRefs.push(...written);
    return Promise.resolve(written.map((w) => (files.has(w) ? [w] : [])));
  },
  promptText: () => prompt,
  refTitle: (paths, line) => `open ${paths.join(",")}:${line ?? ""}`,
  missingTitle: (path) => `not found ${path}`,
  openRef: (target) => opened.push(target),
  forget: () => {
    forgets += 1;
  },
  onFiles: (changed) => {
    filesChanged = changed;
    return () => {
      filesChanged = null;
    };
  },
};

beforeAll(async () => {
  const target = document.createElement("div");
  document.body.append(target);
  core = await createCore({ host: target, looks: host, plugins: [$prose(() => marksPlugin())] });
});

afterAll(async () => {
  await core.destroy();
});

beforeEach(() => {
  vi.useFakeTimers();
  files.clear();
  for (const f of ["there.md", "src/main.rs", "src/lib.rs", "docs/guide.md"]) files.add(f);
  prompt = false;
  askedLinks = [];
  askedRefs = [];
  forgets = 0;
  opened = [];
});

afterEach(() => {
  vi.useRealTimers();
});

const dom = (): HTMLElement => core.view.dom;

async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(LOOKS_DELAY_MS + 10);
  await vi.advanceTimersByTimeAsync(LOOKS_DELAY_MS + 10);
}

async function load(text: string): Promise<void> {
  core.setDoc(core.parse(text));
  await settle();
}

/** The position right after the first occurrence of `text`. */
function after(text: string): number {
  let at = -1;
  core.view.state.doc.descendants((node, pos) => {
    if (at >= 0 || !node.isText) return at < 0;
    const index = (node.text ?? "").indexOf(text);
    if (index >= 0) at = pos + index + text.length;
    return true;
  });
  if (at < 0) throw new Error(`no ${text}`);
  return at;
}

function typeAt(at: number, typed: string): void {
  const view = core.view;
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, at)));
  view.dispatch(view.state.tr.insertText(typed));
}

/** A click on `element` at the document position `pos`, as ProseMirror hands it to `handleClick`. */
function click(element: Element, pos: number, mod: boolean): boolean {
  const event = new MouseEvent("click", { ctrlKey: mod, bubbles: true });
  Object.defineProperty(event, "target", { value: element });
  return core.view.someProp("handleClick", (f) => f(core.view, pos, event)) === true;
}

describe("links to missing files", () => {
  it("get View's missing class on the link and its tooltip; links that lead somewhere do not", async () => {
    await load("[a](gone.md) and [b](there.md) and [c](https://x.org) and [d](#top)");
    const gone = dom().querySelector('a[href="gone.md"]');
    expect(gone?.classList.contains("is-missing")).toBe(true);
    expect(gone?.querySelector("[title]")?.getAttribute("title")).toBe("not found gone.md");
    expect(dom().querySelector('a[href="there.md"]')?.classList.contains("is-missing")).toBe(false);
    expect(dom().querySelectorAll(".is-missing")).toHaveLength(1);
    // only links to files are asked
    expect(askedLinks.sort()).toEqual(["gone.md", "there.md"]);
  });

  it("follow an edited href and files that appear", async () => {
    await load("[a](gone.md) here");
    const view = core.view;
    const link = view.state.schema.marks.link;
    if (!link) throw new Error("no link mark");
    view.dispatch(
      view.state.tr.removeMark(1, 2, link).addMark(1, 2, link.create({ href: "there.md" })),
    );
    await settle();
    expect(dom().querySelector(".is-missing")).toBeNull();
    // back to the missing file, which then appears
    view.dispatch(
      view.state.tr.removeMark(1, 2, link).addMark(1, 2, link.create({ href: "gone.md" })),
    );
    await settle();
    expect(dom().querySelector('a[href="gone.md"]')?.classList.contains("is-missing")).toBe(true);
    files.add("gone.md");
    filesChanged?.();
    await settle();
    expect(dom().querySelector(".is-missing")).toBeNull();
  });

  it("keep the class through typing in the link and its undo; the Markdown stays the same", async () => {
    await load("Read [the gone page](gone.md) now");
    typeAt(after("the"), "x");
    await settle();
    expect(dom().querySelector("a.is-missing")?.textContent).toBe("thex gone page");
    undo(core.view.state, core.view.dispatch);
    await settle();
    expect(dom().querySelector("a.is-missing")?.textContent).toBe("the gone page");
    expect(core.serialize(core.view.state.doc).trim()).toBe("Read [the gone page](gone.md) now");
  });

  it("live alongside correction marks in the same text", async () => {
    const text = "Read [teh gone page](gone.md) now";
    const built = await buildDoc(core, text, [
      { lineStart: 1, lineEnd: 1, kind: "paragraph", lock: null },
    ]);
    core.setDoc(built.doc);
    await settle();
    const from = text.indexOf("teh");
    setMarks(
      core.view,
      compose(core, built, core.view.state.doc),
      [{ from, to: from + 3 }],
      0,
      () => {},
    );
    expect(dom().querySelector("a.is-missing [data-correction-mark]")?.textContent).toBe("teh");
  });
});

describe("code references", () => {
  it("in inline code and plain text get View's chip classes when the file exists", async () => {
    await load("See `src/main.rs:12`, `src/none.rs:1`, `plain` and src/lib.rs:4 or src/none.rs:2.");
    const code = [...dom().querySelectorAll("code")];
    expect(code.map((c) => c.classList.contains("code-ref"))).toEqual([true, false, false]);
    const text = [...dom().querySelectorAll("span.code-ref")];
    expect(text.map((s) => s.textContent)).toEqual(["src/lib.rs:4"]);
    expect(text[0]?.getAttribute("title")).toBe("open src/lib.rs:4");
    expect(code[0]?.querySelector("[title]")?.getAttribute("title")).toBe("open src/main.rs:12");
  });

  it("open on Ctrl | Cmd + click as View's click; a plain click is the caret's", async () => {
    await load("Look at src/lib.rs:4 and `src/main.rs:12:3`.");
    const text = dom().querySelector("span.code-ref");
    const code = dom().querySelector("code.code-ref");
    if (!text || !code) throw new Error("no references");
    expect(click(text, after("src/lib"), false)).toBe(false);
    expect(opened).toEqual([]);
    expect(click(text, after("src/lib"), true)).toBe(true);
    expect(click(code, after("src/main"), true)).toBe(true);
    expect(opened).toEqual([
      { paths: ["src/lib.rs"], line: 4, col: null },
      { paths: ["src/main.rs"], line: 12, col: 3 },
    ]);
    // outside a reference, Ctrl + click is the editor's as before
    expect(click(dom().querySelector("p") ?? dom(), 2, true)).toBe(false);
  });

  it("open the one clicked when two touch", async () => {
    await load("Use `src/main.rs:12`src/lib.rs:4 now.");
    const text = dom().querySelector("span.code-ref");
    const code = dom().querySelector("code.code-ref");
    if (!text || !code) throw new Error("no references");
    // the position between them belongs to both
    const between = after("src/main.rs:12");
    expect(click(text, between, true)).toBe(true);
    expect(click(code, between, true)).toBe(true);
    expect(opened).toEqual([
      { paths: ["src/lib.rs"], line: 4, col: null },
      { paths: ["src/main.rs"], line: 12, col: null },
    ]);
  });
});

describe("heading anchors", () => {
  it("are drawn as View's, outside the text: not focusable, not in the Markdown", async () => {
    const text = "# Title\n\n## Second one\n\nBody";
    await load(text);
    const anchors = [...dom().querySelectorAll("h1 > .anchor, h2 > .anchor")];
    expect(anchors).toHaveLength(2);
    for (const anchor of anchors) {
      expect(anchor.localName).toBe("a");
      expect(anchor.textContent).toBe("#");
      expect(anchor.hasAttribute("href")).toBe(false);
      expect(anchor.hasAttribute("tabindex")).toBe(false);
      // ProseMirror's widget: drawn uneditable, outside the text
      expect(anchor.classList.contains("ProseMirror-widget")).toBe(true);
    }
    expect(core.view.state.doc.textContent).toBe("TitleSecond oneBody");
    expect(core.serialize(core.view.state.doc).trim()).toBe(text);
  });
});

describe("template variables and file references", () => {
  const TEXT = "Hi {{ user }}, read @docs/guide.md and @docs/none.md, not `{{x}}`.";

  it("are marked as View marks them when prompt text is on", async () => {
    prompt = true;
    await load(TEXT);
    expect([...dom().querySelectorAll("span.tvar")].map((e) => e.textContent)).toEqual([
      "{{ user }}",
    ]);
    expect([...dom().querySelectorAll("a.at-ref")].map((e) => e.textContent)).toEqual([
      "@docs/guide.md",
    ]);
    expect(core.serialize(core.view.state.doc).trim()).toBe(TEXT);
  });

  it("are left alone when it is off", async () => {
    await load(TEXT);
    expect(dom().querySelector(".tvar, .at-ref")).toBeNull();
  });

  it("come and go with prompt text switched while editing", async () => {
    await load(TEXT);
    prompt = true;
    repaintLooks(core.view);
    await settle();
    expect(dom().querySelectorAll(".tvar, .at-ref")).toHaveLength(2);
    prompt = false;
    repaintLooks(core.view);
    await settle();
    expect(dom().querySelector(".tvar, .at-ref")).toBeNull();
  });
});

describe("incremental work", () => {
  it("reads only the block that changed and asks nothing it knows", async () => {
    await load("One src/lib.rs:1 [a](gone.md)\n\nTwo plain\n\nThree src/main.rs:2");
    const scans = vi.mocked(findTextRefs);
    scans.mockClear();
    askedLinks = [];
    typeAt(after("Two"), "!");
    await settle();
    expect(scans.mock.calls.map(([text]) => text)).toEqual(["Two! plain"]);
    expect(askedLinks).toEqual([]);
    expect(dom().querySelectorAll(".code-ref")).toHaveLength(2);
    expect(dom().querySelectorAll(".is-missing")).toHaveLength(1);
  });

  it("does nothing when only the caret moves", async () => {
    await load("See `src/main.rs:12` and [a](gone.md) and src/lib.rs:4 here");
    const scans = vi.mocked(findTextRefs);
    scans.mockClear();
    const lookups = vi.spyOn(core.view, "domAtPos");
    try {
      for (const at of [2, 6, 9, 14]) {
        const view = core.view;
        view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, at)));
      }
      await settle();
      expect(lookups).not.toHaveBeenCalled();
      expect(scans).not.toHaveBeenCalled();
      expect(dom().querySelectorAll(".code-ref")).toHaveLength(2);
    } finally {
      lookups.mockRestore();
    }
  });

  it("asks again once per burst of file changes", async () => {
    await load("[a](gone.md) and src/lib.rs:4");
    askedLinks = [];
    askedRefs = [];
    for (let i = 0; i < 3; i += 1) {
      filesChanged?.();
      await vi.advanceTimersByTimeAsync(LOOKS_DELAY_MS / 3);
    }
    await settle();
    expect(forgets).toBe(1);
    expect(askedLinks).toEqual(["gone.md"]);
    expect(askedRefs).toEqual(["src/lib.rs"]);
  });

  it("waits for a composition to end before drawing what was typed", async () => {
    await load("One plain\n\nTwo");
    const composing = vi.spyOn(core.view, "composing", "get").mockReturnValue(true);
    try {
      typeAt(after("plain"), " src/lib.rs:4");
      await settle();
      expect(dom().querySelector(".code-ref")).toBeNull();
      expect(askedRefs).toEqual([]);
    } finally {
      composing.mockRestore();
    }
    await settle();
    expect(dom().querySelector("span.code-ref")?.textContent).toBe("src/lib.rs:4");
  });
});
