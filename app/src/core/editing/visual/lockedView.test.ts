// @vitest-environment jsdom
import "./jsdomShims";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { NodeSelection, TextSelection } from "@milkdown/kit/prose/state";
import type { Body } from "../../../api/generated/Body";
import type { FrontMatter } from "../../../api/generated/FrontMatter";
import { buildDoc, compose, type Built } from "./compose";
import { createCore, LOCKED, type Core, type LockedHost } from "./core";
import { lockedLooks, type LockedRender } from "./lockedRender";
import { appPlugins, type PluginHost } from "./plugins";

let core: Core;
let looks = new WeakMap<PMNode, LockedRender>();
const enhanced: HTMLElement[] = [];
const neared: HTMLElement[] = [];
const undone: HTMLElement[] = [];
const followed: string[] = [];
let refreshed: () => void = () => {};
let opened: { source: string; replace: (source: string) => void }[] = [];

const host: LockedHost = {
  render: (node) => looks.get(node) ?? null,
  refresh: () => {
    refreshed();
    return Promise.resolve();
  },
  blocks: {
    enhance: (element) => {
      enhanced.push(element);
      return element;
    },
    near: (element) => {
      neared.push(element);
      return () => void undone.push(element);
    },
  },
  card: (target, front) => {
    const card = document.createElement("section");
    card.className = "card-stub";
    card.textContent = front.fields.map((f) => f.key).join(",");
    target.append(card);
    return () => card.remove();
  },
  properties: (target, source, replace) => {
    opened.push({ source, replace });
    const sheet = document.createElement("div");
    sheet.className = "sheet-stub";
    sheet.append(document.createElement("input"));
    target.append(sheet);
    return () => sheet.remove();
  },
  reasonText: (reason) => `kept: ${reason}`,
  editLabel: "Source",
  edit: () => {},
};

const plugins: PluginHost = {
  core: () => core,
  changed: () => {},
  selected: () => {},
  save: () => {},
  modeKey: () => {},
  linkKey: () => {},
  slash: () => {},
  slashKey: () => false,
  pictures: () => {},
  followLink: (href) => void followed.push(href),
  displaySrc: (src) => src,
};

beforeAll(async () => {
  const target = document.createElement("div");
  document.body.append(target);
  core = await createCore({ host: target, locked: host, first: appPlugins(plugins) });
});

afterAll(async () => {
  await core.destroy();
});

beforeEach(() => {
  // a document of other nodes: the next one gets node views of its own
  core.setDoc(core.parse("Other text.\n"));
  looks = new WeakMap();
  for (const list of [enhanced, neared, undone, followed]) list.length = 0;
  opened = [];
  refreshed = () => {};
});

const TEXT = [
  "---",
  "title: A plan",
  "---",
  "",
  '<div class="note">',
  'Some <a href="https://example.com/">words</a>',
  "</div>",
  "",
  "[ref]: https://example.com/ref",
  "",
  "Body words here.",
  "",
].join("\n");

const SEGMENTS = [
  { lineStart: 1, lineEnd: 3, kind: "frontMatter", lock: "frontMatter" },
  { lineStart: 5, lineEnd: 7, kind: "html", lock: "html" },
  { lineStart: 9, lineEnd: 9, kind: "raw", lock: "definition" },
  { lineStart: 11, lineEnd: 11, kind: "paragraph", lock: null },
] as const;

const FRONT: FrontMatter = {
  syntax: "yaml",
  raw: "title: A plan",
  fields: [{ key: "title", value: { type: "text", text: "A plan" } }],
};

/** View's render of TEXT, as the backend gives it (each block marked with its id). */
const BODY: Body = {
  kind: "markdown",
  outline: [],
  frontMatter: FRONT,
  blocks: [
    {
      id: "b1",
      kind: "html",
      lineStart: 5,
      lineEnd: 7,
      html: '<div class="note" data-b="b1">\nSome <a href="https://example.com/">words</a>\n</div>',
    },
    {
      id: "b2",
      kind: "p",
      lineStart: 11,
      lineEnd: 11,
      html: '<p data-b="b2">Body words here.</p>',
    },
  ],
};

/** TEXT opened with View's looks known (null body: none known, as after a failed render). */
async function open(body: Body | null = BODY): Promise<Built> {
  const built = await buildDoc(core, TEXT, SEGMENTS);
  if (body !== null) {
    const composed = compose(core, built, built.doc);
    for (const [node, look] of lockedLooks(body, built.doc, composed)) looks.set(node, look);
  }
  core.setDoc(built.doc);
  return built;
}

/** The locked blocks in the editor, in order. */
const blocks = (): HTMLElement[] => [
  ...core.view.dom.querySelectorAll<HTMLElement>(":scope > .pm-locked"),
];

/** Position of the `n`-th locked node. */
function lockedAt(n: number): number {
  const found: number[] = [];
  core.view.state.doc.forEach((node, offset) => {
    if (node.type.name === LOCKED) found.push(offset);
  });
  const at = found[n];
  if (at === undefined) throw new Error(`no locked node ${n}`);
  return at;
}

function selectLocked(n: number): void {
  const view = core.view;
  view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, lockedAt(n))));
}

function caretInText(): void {
  const view = core.view;
  const end = view.state.doc.content.size - 1;
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, end)));
}

function press(
  target: Element,
  type: "mousedown" | "click",
  init: MouseEventInit = {},
): MouseEvent {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, ...init });
  target.dispatchEvent(event);
  return event;
}

describe("a block kept as written, at rest", () => {
  it("shows View's HTML for its lines, without View's block marks, built by View's hooks", async () => {
    await open();
    const html = blocks()[1];
    expect(html?.dataset.look).toBe("view");
    expect(html?.querySelector(":scope > .note a[href]")?.textContent).toBe("words");
    expect(html?.querySelector(".pm-locked__source, .pm-locked__bar")).toBeNull();
    expect(core.view.dom.querySelector("[data-b]")).toBeNull();
    const note = html?.querySelector(".note");
    expect(enhanced).toContain(note);
    expect(neared).toContain(note);
  });

  it("undoes the hooks' work when the block goes", async () => {
    await open();
    const note = blocks()[1]?.querySelector(".note");
    expect(undone).not.toContain(note);
    core.setDoc(core.parse("Other text.\n"));
    expect(undone).toContain(note);
  });

  it("a link definition takes no space; selected from the keyboard, it shows its source", async () => {
    await open();
    const definition = blocks()[2];
    expect(definition?.dataset.look).toBe("none");
    expect(definition?.childElementCount).toBe(0);
    selectLocked(2);
    expect(definition?.dataset.look).toBe("source");
    expect(definition?.querySelector(".pm-locked__source")?.textContent).toBe(
      "[ref]: https://example.com/ref",
    );
    caretInText();
    expect(definition?.dataset.look).toBe("none");
    expect(definition?.childElementCount).toBe(0);
  });

  it("a plain click selects the block (outlined) and does not follow a link in it", async () => {
    await open();
    const html = blocks()[1];
    const link = html?.querySelector("a");
    if (!html || !link) throw new Error("no block");
    const down = press(link, "mousedown");
    const click = press(link, "click");
    const selection = core.view.state.selection;
    expect(selection).toBeInstanceOf(NodeSelection);
    expect(selection.from).toBe(lockedAt(1));
    expect(html.classList.contains("ProseMirror-selectednode")).toBe(true);
    expect(down.defaultPrevented).toBe(true);
    expect(click.defaultPrevented).toBe(true);
    expect(followed).toEqual([]);
  });

  it("Ctrl | Cmd + click on a link follows it, as the editor does in its own text", async () => {
    await open();
    const link = blocks()[1]?.querySelector("a");
    if (!link) throw new Error("no link");
    press(link, "click", { ctrlKey: true });
    press(link, "click", { metaKey: true });
    expect(followed).toEqual(["https://example.com/", "https://example.com/"]);
  });

  it("without a known look (the render failed) every locked block shows its source and the way to Source", async () => {
    await open(null);
    const all = blocks();
    expect(all.map((b) => b.dataset.look)).toEqual(["source", "source", "source"]);
    expect(all[1]?.querySelector(".pm-locked__source")?.textContent).toBe(
      '<div class="note">\nSome <a href="https://example.com/">words</a>\n</div>',
    );
    expect(all[1]?.querySelector(".pm-locked__why")?.textContent).toBe("kept: html");
    expect(all[1]?.querySelector("button")?.textContent).toBe("Source");
    expect(core.view.dom.querySelector(".note")).toBeNull();
  });
});

describe("the front matter", () => {
  it("shows View's card; a click opens the editable properties; Escape brings the card back", async () => {
    await open();
    const front = blocks()[0];
    const card = front?.querySelector(".card-stub");
    if (!front || !card) throw new Error("no card");
    expect(front.dataset.look).toBe("card");
    expect(card.textContent).toBe("title");
    press(card, "mousedown");
    press(card, "click");
    expect(opened.map((o) => o.source)).toEqual(["---\ntitle: A plan\n---"]);
    expect(front.querySelector(".card-stub")).toBeNull();
    expect(front.querySelector(".sheet-stub input")).not.toBeNull();
    // a value set: the properties go on with the new source
    let later: PMNode | null = null;
    refreshed = () => {
      const node = core.view.state.doc.firstChild;
      if (node === null) return;
      later = node;
      looks.set(node, {
        kind: "frontMatter",
        frontMatter: { ...FRONT, fields: [{ key: "name", value: { type: "text", text: "B" } }] },
      });
    };
    opened[0]?.replace("---\nname: B\n---");
    expect(opened.map((o) => o.source)).toEqual(["---\ntitle: A plan\n---", "---\nname: B\n---"]);
    const field = front.querySelector(".sheet-stub input");
    field?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await Promise.resolve();
    expect(later).not.toBeNull();
    expect(front.querySelector(".sheet-stub")).toBeNull();
    expect(front.querySelector(".card-stub")?.textContent).toBe("name");
    expect(core.view.state.selection).toBeInstanceOf(NodeSelection);
  });

  it("Enter on the selected front matter opens the properties; a click outside closes them", async () => {
    await open();
    const front = blocks()[0];
    selectLocked(0);
    core.view.dom.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(front?.querySelector(".sheet-stub")).not.toBeNull();
    const refresh = vi.fn();
    refreshed = refresh;
    document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(front?.querySelector(".sheet-stub")).toBeNull();
    expect(front?.querySelector(".card-stub")).not.toBeNull();
    // nothing changed: nothing to render again
    expect(refresh).not.toHaveBeenCalled();
  });

  it("Enter on another selected locked block leaves it to the editor", async () => {
    await open();
    selectLocked(1);
    core.view.dom.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(opened).toEqual([]);
  });
});
