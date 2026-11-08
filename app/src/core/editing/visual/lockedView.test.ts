// @vitest-environment jsdom
import "./jsdomShims";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { undo } from "@milkdown/kit/prose/history";
import { NodeSelection, TextSelection } from "@milkdown/kit/prose/state";
import type { Body } from "../../../api/generated/Body";
import type { FrontMatter } from "../../../api/generated/FrontMatter";
import { buildDoc, compose, type Built } from "./compose";
import { createCore, LOCKED, type Core, type LockedHost } from "./core";
import { LookBook, type LockedRender } from "./lockedRender";
import { appPlugins, type PluginHost } from "./plugins";

let core: Core;
/** Looks a test sets by node; the rest come from the renders taken, as in the app. */
let looks = new WeakMap<PMNode, LockedRender>();
let book = new LookBook();
const enhanced: HTMLElement[] = [];
const neared: HTMLElement[] = [];
const undone: HTMLElement[] = [];
const followed: string[] = [];
let refreshed: () => void = () => {};
/** When a render asked for comes back (at once unless a test holds it). */
let answer: () => Promise<void> = () => Promise.resolve();
let opened: { source: string; replace: (source: string) => void }[] = [];

const host: LockedHost = {
  render: (node) => looks.get(node) ?? book.get(node),
  refresh: () => {
    refreshed();
    return answer();
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
  sourceLabel: "Source of this block",
  sourceHint: "Esc to finish",
  blockLabel: (name) => `block: ${name}`,
  openHint: "Enter to edit",
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
  core = await createCore({
    host: target,
    locked: host,
    first: appPlugins(plugins),
    label: "Document text",
  });
});

afterAll(async () => {
  await core.destroy();
});

beforeEach(() => {
  // a document of other nodes: the next one gets node views of its own
  core.setDoc(core.parse("Other text.\n"));
  looks = new WeakMap();
  book = new LookBook();
  answer = () => Promise.resolve();
  for (const list of [enhanced, neared, undone, followed]) list.length = 0;
  opened = [];
  refreshed = () => {};
});

afterEach(() => {
  vi.useRealTimers();
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
  if (body !== null) book.take(book.ask(), body, built.doc, compose(core, built, built.doc));
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

  it("a plain click selects the block and opens its source in place; a link in it is not followed", async () => {
    await open();
    const html = blocks()[1];
    const link = html?.querySelector("a");
    if (!html || !link) throw new Error("no block");
    const down = press(link, "mousedown");
    const click = press(link, "click");
    const selection = core.view.state.selection;
    expect(selection).toBeInstanceOf(NodeSelection);
    expect(selection.from).toBe(lockedAt(1));
    expect(down.defaultPrevented).toBe(true);
    expect(click.defaultPrevented).toBe(true);
    expect(followed).toEqual([]);
    expect(html.dataset.look).toBe("open");
    const field = html.querySelector("textarea");
    expect(field?.value).toBe(SOURCE);
    expect(document.activeElement).toBe(field);
    expect(field?.selectionStart).toBe(SOURCE.length);
    expect(field?.getAttribute("aria-label")).toBe("Source of this block");
    const hint = html.querySelector(".pm-locked__hint");
    expect(hint?.textContent).toBe("Esc to finish");
    expect(field?.getAttribute("aria-describedby")).toBe(hint?.id);
    // below the box: View's look of what it holds, hydrated as View's, never clicked or focused
    const preview = html.querySelector(".pm-locked__preview");
    expect(preview?.hasAttribute("inert")).toBe(true);
    const note = preview?.querySelector(":scope > .note");
    expect(note?.textContent).toContain("words");
    expect(enhanced).toContain(note);
    expect(neared).toContain(note);
  });

  it("Ctrl | Cmd + click on a link follows it, as the editor does in its own text, and opens nothing", async () => {
    await open();
    const link = blocks()[1]?.querySelector("a");
    if (!link) throw new Error("no link");
    press(link, "click", { ctrlKey: true });
    press(link, "click", { metaKey: true });
    expect(followed).toEqual(["https://example.com/", "https://example.com/"]);
    expect(core.view.dom.querySelector("textarea")).toBeNull();
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
    expect(front.dataset.look).toBe("properties");
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

  it("Enter on another selected locked block opens its source, not the properties", async () => {
    await open();
    selectLocked(1);
    core.view.dom.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(opened).toEqual([]);
    expect(blocks()[1]?.querySelector("textarea")?.value).toBe(SOURCE);
  });

  it("without editable properties on offer, its source opens in the box", async () => {
    await open();
    const properties = host.properties;
    host.properties = () => null;
    try {
      const front = blocks()[0];
      const card = front?.querySelector(".card-stub");
      if (!front || !card) throw new Error("no card");
      press(card, "mousedown");
      press(card, "click");
      expect(front.querySelector("textarea")?.value).toBe("---\ntitle: A plan\n---");
    } finally {
      host.properties = properties;
    }
  });
});

/** The HTML block of TEXT. */
const SOURCE = '<div class="note">\nSome <a href="https://example.com/">words</a>\n</div>';

const MATH_TEXT = [
  "Intro words.",
  "",
  "$$",
  "a + b",
  "$$",
  "",
  "",
  "$$",
  "c",
  "$$",
  "",
  "End words.",
  "",
].join("\n");

const MATH_SEGMENTS = [
  { lineStart: 1, lineEnd: 1, kind: "paragraph", lock: null },
  { lineStart: 3, lineEnd: 5, kind: "math", lock: "math" },
  { lineStart: 8, lineEnd: 10, kind: "math", lock: "math" },
  { lineStart: 12, lineEnd: 12, kind: "paragraph", lock: null },
] as const;

/** View's look of a math block: the backend's HTML for `formula`. */
const mathLook = (formula: string): LockedRender => ({
  kind: "html",
  html: `<div class="math" data-b="m">${formula}</div>`,
});

/** MATH_TEXT opened, each math block with View's look. */
async function openMath(): Promise<Built> {
  const built = await buildDoc(core, MATH_TEXT, MATH_SEGMENTS);
  const formulas = ["a + b", "c"];
  built.doc.forEach((node) => {
    if (node.type.name === LOCKED) looks.set(node, mathLook(formulas.shift() ?? ""));
  });
  core.setDoc(built.doc);
  return built;
}

/** Opens the `n`-th locked block with a click; its source box. */
function openBox(n: number): HTMLTextAreaElement {
  const block = blocks()[n];
  if (!block) throw new Error(`no block ${n}`);
  press(block, "mousedown");
  press(block, "click");
  const field = block.querySelector("textarea");
  if (!field) throw new Error("not opened");
  return field;
}

/** The person's input in the box. */
function type(field: HTMLTextAreaElement, text: string, init: InputEventInit = {}): void {
  field.value = text;
  field.dispatchEvent(new InputEvent("input", { bubbles: true, ...init }));
}

function key(field: Element, name: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key: name,
    bubbles: true,
    cancelable: true,
    ...init,
  });
  field.dispatchEvent(event);
  return event;
}

const sourceAt = (n: number): string =>
  String(core.view.state.doc.nodeAt(lockedAt(n))?.attrs.source);

/** The next frame: what the editor measures in it is done by then. */
const frame = (): Promise<void> => new Promise((done) => requestAnimationFrame(() => done()));

/** Lets the renders asked for settle (the test host answers at once). */
async function settle(): Promise<void> {
  for (let i = 0; i < 3; i++) await Promise.resolve();
}

describe("a block opened in place", () => {
  it("what is typed goes into the document at once; only that block's bytes change; undo in the box restores them", async () => {
    const built = await openMath();
    const field = openBox(0);
    type(field, "$$\na + b + c\n$$");
    const composed = compose(core, built, core.view.state.doc);
    expect(composed.text).toBe(MATH_TEXT.replace("a + b", "a + b + c"));
    expect(composed.changed).toBe(1);
    type(field, "$$\na + b + c + d\n$$");
    // the box is kept through the edits: same element, still focused
    expect(blocks()[0]?.querySelector("textarea")).toBe(field);
    expect(document.activeElement).toBe(field);
    // keys in the box are its own: Backspace never deletes the block
    key(field, "Backspace");
    expect(core.view.state.doc.nodeAt(lockedAt(0))?.type.name).toBe(LOCKED);
    // the editor's undo: quick edits are one step, and the original block comes back byte for byte
    const undone = key(field, "z", { ctrlKey: true });
    expect(undone.defaultPrevented).toBe(true);
    expect(field.value).toBe("$$\na + b\n$$");
    const back = compose(core, built, core.view.state.doc);
    expect(back.text).toBe(MATH_TEXT);
    expect(back.changed).toBe(0);
    key(field, "z", { ctrlKey: true, shiftKey: true });
    expect(field.value).toBe("$$\na + b + c + d\n$$");
    expect(sourceAt(0)).toBe("$$\na + b + c + d\n$$");
    key(field, "z", { metaKey: true });
    key(field, "y", { ctrlKey: true });
    expect(sourceAt(0)).toBe("$$\na + b + c + d\n$$");
    expect(blocks()[0]?.querySelector("textarea")).toBe(field);
  });

  it("the preview shows View's render of the typed text once typing pauses; a render without it keeps the last", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    await openMath();
    const field = openBox(0);
    const preview = blocks()[0]?.querySelector(".pm-locked__preview");
    const first = preview?.querySelector(".math");
    expect(first?.textContent).toBe("a + b");
    const refresh = vi.fn(() => {
      const node = core.view.state.doc.nodeAt(lockedAt(0));
      if (node) looks.set(node, mathLook(String(node.attrs.source).split("\n")[1] ?? ""));
    });
    refreshed = refresh;
    type(field, "$$\nx\n$$");
    type(field, "$$\nx + y\n$$");
    vi.advanceTimersByTime(249);
    expect(refresh).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    await settle();
    expect(refresh).toHaveBeenCalledTimes(1);
    const next = preview?.querySelector(".math");
    expect(next?.textContent).toBe("x + y");
    expect(undone).toContain(first);
    expect(enhanced).toContain(next);
    // the render failed | was not sure: the last preview stays
    refreshed = () => {};
    type(field, "$$\nz\n$$");
    vi.advanceTimersByTime(250);
    await settle();
    expect(preview?.querySelector(".math")?.textContent).toBe("x + y");
  });

  it("Escape closes the box: View's look of the new source, the block selected", async () => {
    await openMath();
    const field = openBox(0);
    refreshed = () => {
      const node = core.view.state.doc.nodeAt(lockedAt(0));
      if (node) looks.set(node, mathLook("x"));
    };
    type(field, "$$\nx\n$$");
    const escape = key(field, "Escape");
    expect(escape.defaultPrevented).toBe(true);
    await settle();
    const block = blocks()[0];
    expect(block?.querySelector("textarea")).toBeNull();
    expect(block?.dataset.look).toBe("view");
    expect(block?.querySelector(":scope > .math")?.textContent).toBe("x");
    const selection = core.view.state.selection;
    expect(selection).toBeInstanceOf(NodeSelection);
    expect(selection.from).toBe(lockedAt(0));
    // the other block keeps its look
    expect(blocks()[1]?.querySelector(":scope > .math")?.textContent).toBe("c");
  });

  it("focus moving elsewhere closes it; the window losing focus does not", async () => {
    await openMath();
    const field = openBox(1);
    // the window lost focus: the box still has it
    field.dispatchEvent(new FocusEvent("blur"));
    expect(blocks()[1]?.querySelector("textarea")).toBe(field);
    field.blur();
    expect(blocks()[1]?.querySelector("textarea")).toBeNull();
    expect(blocks()[1]?.dataset.look).toBe("view");
  });

  it("an arrow key out of the first | last line closes it and moves the caret before | after the block", async () => {
    await openMath();
    let field = openBox(0);
    field.setSelectionRange(0, 0);
    expect(key(field, "ArrowUp", { shiftKey: true }).defaultPrevented).toBe(false);
    expect(key(field, "ArrowUp").defaultPrevented).toBe(true);
    expect(blocks()[0]?.querySelector("textarea")).toBeNull();
    const before = core.view.state.selection;
    expect(before).toBeInstanceOf(TextSelection);
    expect(before.$from.parent.textContent).toBe("Intro words.");
    expect(before.$from.parentOffset).toBe("Intro words.".length);
    field = openBox(0);
    field.setSelectionRange(3, 3);
    expect(key(field, "ArrowDown").defaultPrevented).toBe(false);
    expect(blocks()[0]?.querySelector("textarea")).toBe(field);
    field.setSelectionRange(field.value.length, field.value.length);
    key(field, "ArrowDown");
    expect(blocks()[0]?.querySelector("textarea")).toBeNull();
    // the next block is a locked one: it is selected, as an arrow key in the editor does
    const after = core.view.state.selection;
    expect(after).toBeInstanceOf(NodeSelection);
    expect(after.from).toBe(lockedAt(1));
  });

  it("an emptied block is removed on close; one undo brings it back byte for byte", async () => {
    const built = await openMath();
    const field = openBox(0);
    type(field, "  \n");
    key(field, "Escape");
    expect(blocks()).toHaveLength(1);
    expect(compose(core, built, core.view.state.doc).text).toBe(
      "Intro words.\n\n$$\nc\n$$\n\nEnd words.\n",
    );
    undo(core.view.state, core.view.dispatch);
    const back = compose(core, built, core.view.state.doc);
    expect(back.text).toBe(MATH_TEXT);
    expect(back.changed).toBe(0);
  });

  it("an IME composition goes into the document only once it is chosen", async () => {
    await openMath();
    const field = openBox(0);
    field.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    type(field, "$$\na + b か\n$$", { isComposing: true });
    expect(sourceAt(0)).toBe("$$\na + b\n$$");
    // Escape ends the composition, not the box
    key(field, "Escape", { isComposing: true });
    expect(blocks()[0]?.querySelector("textarea")).toBe(field);
    type(field, "$$\na + b 化\n$$", { isComposing: true });
    field.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "化" }));
    expect(sourceAt(0)).toBe("$$\na + b 化\n$$");
    expect(blocks()[0]?.querySelector("textarea")).toBe(field);
  });

  it("a link definition selected from the keyboard opens with Enter", async () => {
    await open();
    selectLocked(2);
    core.view.dom.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    const definition = blocks()[2];
    expect(definition?.querySelector("textarea")?.value).toBe("[ref]: https://example.com/ref");
    // View draws nothing for it: an empty preview
    expect(definition?.querySelector(".pm-locked__preview")?.childElementCount).toBe(0);
  });
});

const PROMPT_TEXT = [
  "<context>",
  "Inner words.",
  "",
  "$$",
  "x",
  "$$",
  "",
  "</context>",
  "",
  "After.",
  "",
  "<context>",
  "",
  "Tail.",
  "",
].join("\n");

const PROMPT_SEGMENTS = [
  { lineStart: 1, lineEnd: 1, kind: "html", lock: "html" },
  { lineStart: 2, lineEnd: 2, kind: "paragraph", lock: null },
  { lineStart: 4, lineEnd: 6, kind: "math", lock: "math" },
  { lineStart: 8, lineEnd: 8, kind: "html", lock: "html" },
  { lineStart: 10, lineEnd: 10, kind: "paragraph", lock: null },
  { lineStart: 12, lineEnd: 12, kind: "html", lock: "html" },
  { lineStart: 14, lineEnd: 14, kind: "paragraph", lock: null },
] as const;

const PROMPT_MATH = '<p><span data-math-style="display">x</span></p>';

/** View's render of PROMPT_TEXT: one box for the paired tags; the unpaired one is dropped by the sanitizer. */
const PROMPT_BODY: Body = {
  kind: "markdown",
  outline: [],
  frontMatter: null,
  blocks: [
    {
      id: "d1",
      kind: "details",
      lineStart: 2,
      lineEnd: 6,
      html:
        '<details data-b="d1" class="prompt-block" data-tag="context" open=""><summary>context</summary>\n' +
        `<p>Inner words.</p>\n${PROMPT_MATH}\n</details>`,
    },
    { id: "p1", kind: "p", lineStart: 10, lineEnd: 10, html: '<p data-b="p1">After.</p>' },
    { id: "p2", kind: "p", lineStart: 14, lineEnd: 14, html: '<p data-b="p2">Tail.</p>' },
  ],
};

async function openPrompt(): Promise<Built> {
  const built = await buildDoc(core, PROMPT_TEXT, PROMPT_SEGMENTS);
  book.take(book.ask(), PROMPT_BODY, built.doc, compose(core, built, built.doc));
  core.setDoc(built.doc);
  return built;
}

/** The top-level blocks of the editor, in order. */
const tops = (): HTMLElement[] => [...core.view.dom.querySelectorAll<HTMLElement>(":scope > *")];

const inBox = (el: Element | undefined): boolean => el?.classList.contains("pm-prompt-in") ?? false;

describe("a prompt box", () => {
  it("its tag lines are the box's edges, the blocks between get the box's layout and stay the editor's own", async () => {
    await openPrompt();
    const [open, inner, math, close, after, lone, tail] = tops();
    expect(open?.dataset.look).toBe("prompt");
    expect(open?.dataset.edge).toBe("open");
    const summary = open?.querySelector(":scope > details.prompt-block[open] > summary");
    expect(summary?.textContent).toBe("context");
    expect(summary?.childElementCount).toBe(0);
    expect(open?.querySelector<HTMLElement>(":scope > details")?.dataset.tag).toBe("context");
    // the frame paints the box: there, stretched to the close edge
    expect(open?.querySelector(":scope > .pm-prompt-frame")).not.toBeNull();
    expect(open?.dataset.box).toBe("");
    expect(open?.style.getPropertyValue("--prompt-box")).toBe("0px");
    expect(close?.dataset.look).toBe("prompt");
    expect(close?.dataset.edge).toBe("close");
    expect(close?.childElementCount).toBe(0);

    expect(inner?.localName).toBe("p");
    expect(inBox(inner)).toBe(true);
    expect(inner?.classList.contains("pm-prompt-end")).toBe(false);
    // a kept block inside: its own View element, never the box's markup
    expect(math?.dataset.look).toBe("view");
    expect(inBox(math)).toBe(true);
    expect(math?.classList.contains("pm-prompt-end")).toBe(true);
    expect(math?.querySelector(":scope > p > [data-math-style]")).not.toBeNull();
    expect(math?.querySelector("details, summary")).toBeNull();

    // outside the box, and a tag line without its pair: no box
    expect([after, lone, tail].some(inBox)).toBe(false);
    expect(lone?.dataset.edge).toBeUndefined();
    expect(core.view.dom.querySelectorAll(".pm-prompt-frame")).toHaveLength(1);
  });

  it("the frame reaches from the top of the open edge to the bottom of the close edge", async () => {
    await openPrompt();
    const [open, , , close] = tops();
    if (!open || !close) throw new Error("no box");
    open.getBoundingClientRect = () => new DOMRect(0, 100, 600, 40);
    close.getBoundingClientRect = () => new DOMRect(0, 251, 600, 9);
    caretInText();
    core.view.dispatch(core.view.state.tr.insertText("!"));
    await frame();
    expect(open.style.getPropertyValue("--prompt-box")).toBe("160px");
  });

  it("typing in a block inside: it keeps the box's layout, and only its bytes change", async () => {
    const built = await openPrompt();
    const view = core.view;
    let end = -1;
    view.state.doc.forEach((node, offset) => {
      if (node.textContent === "Inner words.") end = offset + node.nodeSize - 1;
    });
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, end)));
    view.dispatch(view.state.tr.insertText(" More."));
    const inner = tops()[1];
    expect(inner?.textContent).toBe("Inner words. More.");
    expect(inBox(inner)).toBe(true);
    const composed = compose(core, built, view.state.doc);
    expect(composed.text).toBe(PROMPT_TEXT.replace("Inner words.", "Inner words. More."));
    expect(composed.changed).toBe(1);
  });

  it("a click on a tag line opens its source in place, the box still drawn; Escape brings the edge back", async () => {
    await openPrompt();
    const open = tops()[0];
    const summary = open?.querySelector("summary");
    if (!open || !summary) throw new Error("no edge");
    press(summary, "mousedown");
    const click = press(summary, "click");
    // the box never folds in the editor
    expect(click.defaultPrevented).toBe(true);
    expect(open.dataset.look).toBe("open");
    const field = open.querySelector("textarea");
    expect(field?.value).toBe("<context>");
    expect(open.querySelector(":scope > .pm-prompt-frame")).not.toBeNull();
    expect(inBox(tops()[1])).toBe(true);
    if (!field) return;
    key(field, "Escape");
    expect(open.dataset.look).toBe("prompt");
    expect(open.querySelector("summary")?.textContent).toBe("context");
  });

  it("Enter on a selected close tag line opens its source", async () => {
    await openPrompt();
    selectLocked(2);
    core.view.dom.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(tops()[3]?.querySelector("textarea")?.value).toBe("</context>");
  });

  it("while a tag line is typed in, the box stays; once its new look comes back, the box follows it", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    await openPrompt();
    const field = openBox(2);
    type(field, "</contexts>");
    expect(inBox(tops()[1])).toBe(true);
    // View's render of the new text: the tag line pairs with nothing, the box is gone
    refreshed = () => {
      for (const n of [0, 2]) {
        const node = core.view.state.doc.nodeAt(lockedAt(n));
        if (node) looks.set(node, { kind: "none" });
      }
    };
    vi.advanceTimersByTime(250);
    await settle();
    expect(inBox(tops()[1])).toBe(false);
    await frame();
    expect(tops()[0]?.dataset.box).toBeUndefined();
  });

  it("deleting a tag line takes the box away; undo brings it back", async () => {
    await openPrompt();
    const view = core.view;
    const at = lockedAt(2);
    view.dispatch(view.state.tr.delete(at, at + 1));
    expect(tops().some(inBox)).toBe(false);
    await frame();
    expect(tops()[0]?.dataset.box).toBeUndefined();
    undo(view.state, view.dispatch);
    expect(inBox(tops()[1])).toBe(true);
    await frame();
    expect(tops()[0]?.dataset.box).toBe("");
  });
});

describe("a prompt box while typing elsewhere", () => {
  it("typing outside the box finds no box again and measures nothing at once; the next frame measures once", async () => {
    await openPrompt();
    const open = tops()[0];
    if (!open) throw new Error("no box");
    const render = vi.spyOn(host, "render");
    const measured = vi.spyOn(open, "getBoundingClientRect");
    const view = core.view;
    let end = -1;
    view.state.doc.forEach((node, offset) => {
      if (node.textContent === "After.") end = offset + node.nodeSize - 1;
    });
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, end)));
    view.dispatch(view.state.tr.insertText(" More."));
    view.dispatch(view.state.tr.insertText("!"));
    try {
      expect(render).not.toHaveBeenCalled();
      expect(measured).not.toHaveBeenCalled();
      expect(inBox(tops()[1])).toBe(true);
      expect(inBox(tops()[4])).toBe(false);
      await frame();
      expect(measured).toHaveBeenCalledTimes(1);
      // a moved caret lays nothing out again
      caretInText();
      await frame();
      expect(measured).toHaveBeenCalledTimes(1);
    } finally {
      render.mockRestore();
    }
  });
});

const NOTES_TEXT = ["Intro.", "", "[^a]: Note A.", "", "[^b]: Note B.", ""].join("\n");

const NOTES_SEGMENTS = [
  { lineStart: 1, lineEnd: 1, kind: "paragraph", lock: null },
  { lineStart: 3, lineEnd: 3, kind: "footnote", lock: "footnote" },
  { lineStart: 5, lineEnd: 5, kind: "footnote", lock: "footnote" },
] as const;

const note = (label: string, first: boolean): LockedRender => ({
  kind: "footnote",
  html: `<section class="footnotes"><ol><li id="fn-${label}"><p>Note ${label}.</p></li></ol></section>`,
  first,
});

const REF_TEXT = ["See [the site][ref].", "", "[ref]: https://example.com/a", ""].join("\n");

const REF_SEGMENTS = [
  { lineStart: 1, lineEnd: 1, kind: "paragraph", lock: null },
  { lineStart: 3, lineEnd: 3, kind: "raw", lock: "definition" },
] as const;

const linkTo = (href: string): LockedRender => ({
  kind: "html",
  html: `<p>See <a href="${href}">the site</a>.</p>`,
});

describe("blocks at rest follow View's look of the text around them", () => {
  it("deleting the first footnote: the next one becomes the first, View's rule above it", async () => {
    const built = await buildDoc(core, NOTES_TEXT, NOTES_SEGMENTS);
    looks.set(built.doc.child(1), note("a", true));
    looks.set(built.doc.child(2), note("b", false));
    core.setDoc(built.doc);
    expect(blocks()[1]?.dataset.notes).toBe("next");
    const asked = vi.fn(() => {
      const node = core.view.state.doc.nodeAt(lockedAt(0));
      if (node) looks.set(node, note("b", true));
    });
    refreshed = asked;
    const view = core.view;
    const at = lockedAt(0);
    view.dispatch(view.state.tr.delete(at, at + 1));
    await settle();
    expect(asked).toHaveBeenCalledTimes(1);
    expect(blocks()).toHaveLength(1);
    expect(blocks()[0]?.dataset.notes).toBe("first");
  });

  it("editing a link definition: the paragraph using it follows; a block whose look did not change stays as drawn", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const built = await buildDoc(core, REF_TEXT, REF_SEGMENTS);
    const paragraph = built.doc.child(0);
    expect(paragraph.type.name).toBe(LOCKED);
    looks.set(paragraph, linkTo("https://example.com/a"));
    looks.set(built.doc.child(1), { kind: "none" });
    core.setDoc(built.doc);
    const field = openBox(1);
    refreshed = () => {
      looks.set(core.view.state.doc.child(0), linkTo("https://example.com/b"));
    };
    type(field, "[ref]: https://example.com/b");
    vi.advanceTimersByTime(250);
    await settle();
    expect(blocks()[0]?.querySelector("a")?.getAttribute("href")).toBe("https://example.com/b");
    // the same look again: nothing drawn anew
    const drawn = blocks()[0]?.querySelector("p");
    type(field, "[ref]: https://example.com/b ");
    vi.advanceTimersByTime(250);
    await settle();
    expect(blocks()[0]?.querySelector("p")).toBe(drawn);
  });

  it("undo after an edit was closed: the block shows View's look of its old source at once", async () => {
    await open();
    const field = openBox(1);
    type(field, '<div class="note">\nOther words\n</div>');
    key(field, "Escape");
    await settle();
    // no render of the new text came back: its source shows
    expect(blocks()[1]?.dataset.look).toBe("source");
    undo(core.view.state, core.view.dispatch);
    expect(blocks()[1]?.dataset.look).toBe("view");
    expect(blocks()[1]?.querySelector(":scope > .note")?.textContent).toContain("words");
  });

  it("a block brought back by undo with no look known: a render is asked for, its look comes with it", async () => {
    await openMath();
    const view = core.view;
    const at = lockedAt(0);
    view.dispatch(view.state.tr.delete(at, at + 1));
    await settle();
    const asked = vi.fn(() => {
      const node = core.view.state.doc.nodeAt(lockedAt(0));
      if (node && !looks.has(node)) looks.set(node, mathLook("a + b"));
    });
    refreshed = asked;
    undo(view.state, view.dispatch);
    await settle();
    expect(asked).toHaveBeenCalledTimes(1);
    expect(blocks()[0]?.dataset.look).toBe("view");
    expect(blocks()[0]?.querySelector(".math")?.textContent).toBe("a + b");
  });
});

describe("renders that come back late", () => {
  it("an answer for a text since typed on never overwrites the preview", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    await openMath();
    const field = openBox(0);
    const held: (() => void)[] = [];
    answer = () => new Promise<void>((done) => void held.push(done));
    refreshed = () => {
      const node = core.view.state.doc.nodeAt(lockedAt(0));
      if (node) looks.set(node, mathLook(String(node.attrs.source).split("\n")[1] ?? ""));
    };
    const preview = blocks()[0]?.querySelector(".pm-locked__preview");
    type(field, "$$\nold\n$$");
    vi.advanceTimersByTime(250);
    expect(held).toHaveLength(1);
    type(field, "$$\nnew\n$$");
    held[0]?.();
    await settle();
    expect(preview?.querySelector(".math")?.textContent).toBe("a + b");
    vi.advanceTimersByTime(250);
    held[1]?.();
    await settle();
    expect(preview?.querySelector(".math")?.textContent).toBe("new");
  });

  it("a block gone with typing still to render, or with its render on the way: nothing comes after", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    await openMath();
    const asked = vi.fn();
    refreshed = asked;
    type(openBox(0), "$$\nx\n$$");
    // another document opens
    core.setDoc(core.parse("Other text.\n"));
    vi.advanceTimersByTime(1000);
    await settle();
    expect(asked).not.toHaveBeenCalled();

    await openMath();
    const held: (() => void)[] = [];
    answer = () => new Promise<void>((done) => void held.push(done));
    type(openBox(0), "$$\ny\n$$");
    vi.advanceTimersByTime(250);
    expect(asked).toHaveBeenCalledTimes(1);
    const preview = blocks()[0]?.querySelector(".pm-locked__preview");
    const dispatch = vi.spyOn(core.view, "dispatch");
    try {
      core.setDoc(core.parse("Other text.\n"));
      held[0]?.();
      await settle();
      vi.advanceTimersByTime(1000);
      expect(dispatch).not.toHaveBeenCalled();
      expect(preview?.querySelector(".math")?.textContent).toBe("a + b");
      expect(asked).toHaveBeenCalledTimes(1);
    } finally {
      dispatch.mockRestore();
    }
  });
});

describe("safety and input", () => {
  it("while a block is open, a link in its preview is never followed (not every webview knows inert)", async () => {
    await open();
    openBox(1);
    const link = blocks()[1]?.querySelector(".pm-locked__preview a[href]");
    if (!link) throw new Error("no link");
    expect(press(link, "click").defaultPrevented).toBe(true);
    expect(press(link, "click", { ctrlKey: true }).defaultPrevented).toBe(true);
    expect(followed).toEqual([]);
  });

  it("a key the IME takes (key code 229) is never one of the box's own", async () => {
    await openMath();
    const field = openBox(0);
    field.setSelectionRange(0, 0);
    expect(key(field, "ArrowUp", { keyCode: 229 }).defaultPrevented).toBe(false);
    expect(key(field, "Escape", { keyCode: 229 }).defaultPrevented).toBe(false);
    expect(blocks()[0]?.querySelector("textarea")).toBe(field);
  });

  it("a change of the block during a composition leaves the box's text alone; the composition's end puts it in", async () => {
    await openMath();
    const field = openBox(0);
    field.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    type(field, "$$\na + b か\n$$", { isComposing: true });
    const view = core.view;
    const at = lockedAt(0);
    const node = view.state.doc.nodeAt(at);
    view.dispatch(
      view.state.tr.setNodeMarkup(at, undefined, { ...node?.attrs, source: "$$\nq\n$$" }),
    );
    expect(field.value).toBe("$$\na + b か\n$$");
    type(field, "$$\na + b 化\n$$", { isComposing: true });
    field.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "化" }));
    expect(sourceAt(0)).toBe("$$\na + b 化\n$$");
    expect(field.value).toBe("$$\na + b 化\n$$");
  });
});

describe("names for assistive technology", () => {
  it("each block kept as written is a named group that says how to open it; the editor's text has a name", async () => {
    expect(core.view.dom.getAttribute("aria-label")).toBe("Document text");
    await open();
    const [front, html, definition] = blocks();
    for (const block of [front, html, definition]) {
      expect(block?.getAttribute("role")).toBe("group");
      expect(block?.getAttribute("aria-description")).toBe("Enter to edit");
    }
    expect(front?.getAttribute("aria-label")).toBe("block: properties");
    expect(html?.getAttribute("aria-label")).toBe("block: html");
    expect(definition?.getAttribute("aria-label")).toBe("block: other");
    // opened: the box has its own name and hint
    openBox(1);
    expect(html?.hasAttribute("aria-description")).toBe(false);
    await openMath();
    expect(blocks()[0]?.getAttribute("aria-label")).toBe("block: formula");
  });
});
