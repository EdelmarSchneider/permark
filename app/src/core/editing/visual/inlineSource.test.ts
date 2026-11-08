// @vitest-environment jsdom
import "./jsdomShims";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { undo } from "@milkdown/kit/prose/history";
import { NodeSelection, TextSelection } from "@milkdown/kit/prose/state";
import type { Segment } from "../../../api/generated/Segment";
import { classify } from "./classify";
import { buildDoc, compose, type Built } from "./compose";
import { createCore, INLINE_SOURCE, LOCKED, relook, type Core, type LockedHost } from "./core";
import { appPlugins, type PluginHost } from "./plugins";

let core: Core;
const neared: HTMLElement[] = [];
const undone: HTMLElement[] = [];
const followed: string[] = [];
/** View's elements for HTML pieces by source (what a render of the document gave) and the renders asked for. */
const viewHtml = new Map<string, string>();
let renders = 0;

const host: LockedHost = {
  render: () => null,
  refresh: () => {
    renders += 1;
    return Promise.resolve();
  },
  inline: (source) => viewHtml.get(source) ?? null,
  blocks: {
    enhance: (element) => element,
    near: (element) => {
      neared.push(element);
      return () => void undone.push(element);
    },
  },
  card: () => null,
  properties: () => null,
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
  core = await createCore({ host: target, locked: host, first: appPlugins(plugins) });
});

afterAll(async () => {
  await core.destroy();
});

beforeEach(() => {
  core.setDoc(core.parse("Other text.\n"));
  neared.length = 0;
  undone.length = 0;
  followed.length = 0;
});

/** The inline atoms the editor reads in `text`, by their source. */
function atomsIn(text: string): string[] {
  const found: string[] = [];
  core.parse(text).descendants((node) => {
    if (node.type.name === INLINE_SOURCE) found.push(String(node.attrs.source));
  });
  return found;
}

/** Blank-line separated runs (test texts only), as in compose.test.ts. */
function segmentsOf(text: string): Segment[] {
  const lines = text.split("\n");
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

async function load(text: string): Promise<Built> {
  const built = await buildDoc(core, text, segmentsOf(text));
  core.setDoc(built.doc);
  return built;
}

/** Position of the `n`-th inline atom. */
function atomAt(n = 0): number {
  const found: number[] = [];
  core.view.state.doc.descendants((node, pos) => {
    if (node.type.name === INLINE_SOURCE) found.push(pos);
  });
  const at = found[n];
  if (at === undefined) throw new Error(`no atom ${n}`);
  return at;
}

const atoms = (): HTMLElement[] => [...core.view.dom.querySelectorAll<HTMLElement>(".pm-inline")];

const sourceAt = (n = 0): string => String(core.view.state.doc.nodeAt(atomAt(n))?.attrs.source);

function press(
  target: Element,
  type: "mousedown" | "click" | "auxclick",
  init: MouseEventInit = {},
): void {
  target.dispatchEvent(
    new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, ...init }),
  );
}

/** Opens the `n`-th atom with a click; its source field. */
function openAtom(n = 0): HTMLInputElement {
  const atom = atoms()[n];
  if (!atom) throw new Error(`no atom ${n}`);
  press(atom, "mousedown");
  press(atom, "click");
  const field = atom.querySelector("input");
  if (!field) throw new Error("not opened");
  return field;
}

function type(field: HTMLInputElement, text: string, init: InputEventInit = {}): void {
  field.value = text;
  field.dispatchEvent(new InputEvent("input", { bubbles: true, ...init }));
}

function key(target: Element, name: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key: name,
    bubbles: true,
    cancelable: true,
    ...init,
  });
  target.dispatchEvent(event);
  return event;
}

describe("inline math, read as View reads it", () => {
  it.each([
    ["Energy $E=mc^2$ here", ["$E=mc^2$"]],
    ["Prices $5 and $6 today", []],
    ["Cost $x$5 dollars", []],
    ["Spaces $ x$ and $x $ inside", []],
    ["Escaped \\$x$ sign", []],
    ["Kept $a\\$b$ dollar", ["$a\\$b$"]],
    ["Display $$x^2$$ inline", ["$$x^2$$"]],
    ["Code math $`x^2`$ here", ["$`x^2`$"]],
    ["Code `$x$` span", []],
    ["Subscripts $a_b_c$ stay", ["$a_b_c$"]],
    ["Three $$$x$$$ signs", []],
    ["Twins $x$$y$ here", ["$x$", "$y$"]],
    ["Link [a $x$ b](u.md) text", ["$x$"]],
    ["Odd $x$$$y$ z", ["$x$"]],
    ["Empty code $``$ x", []],
    ["Code open $`a $ b`$ c", ["$`a $ b`$"]],
    ["Dd $$a$b$$ e", ["$$a$b$$"]],
    ["Esc \\$$x$ f", ["$x$"]],
    ["Back $a\\\\$ b$ c", ["$a\\\\$ b$"]],
    ["Emph *a $b* c$ d", ["$b* c$"]],
    ["I paid $5, he paid 10$ ok", ["$5, he paid 10$"]],
    ["Auto <http://a.b/$x$> $y$", ["$y$"]],
    // the element is a piece of its own (HTML written in a line): no formula inside its attribute
    ['Html <span title="$x$">t</span> $y$', ['<span title="$x$">t</span>', "$y$"]],
  ])("%s", (text, expected) => {
    expect(atomsIn(text)).toEqual(expected);
  });

  it("each atom is written back as its own source", () => {
    const text = "A $a_b_c$ and $$x$$, $`y`$ and \\$5 $q\\$r$ [l $x$](u.md) *e $z$* end.";
    const doc = core.parse(text);
    expect(core.write(doc.child(0))).toBe(text);
  });
});

describe("blocks with inline math", () => {
  it.each([
    ["a paragraph", "Energy $E=mc^2$ here."],
    ["a heading", "## Sum $a+b$ rule"],
    ["a list", "- item $x_1$\n- two $y$"],
    ["a table cell", "| a | b |\n| --- | --- |\n| $x$ | 2 |"],
  ])("%s is editable text", (_, text) => {
    const verdict = classify(core, text, null, false);
    expect(verdict.editable).toBe(true);
  });

  it("math across lines keeps its block as written", () => {
    expect(classify(core, "Energy $E=\nmc^2$ here.", null, false)).toEqual({
      editable: false,
      reason: "math",
    });
  });
});

describe("an inline formula at rest", () => {
  it("is View's math element, hydrated by View's hooks and undone when it goes", async () => {
    await load("Energy $E=mc^2$, $$x^2$$ and $`y`$ here.\n");
    const types: string[] = [];
    core.view.state.doc.forEach((node) => types.push(node.type.name));
    expect(types).toEqual(["paragraph"]);
    const looks = atoms().map((atom) => atom.querySelector("[data-math-style]"));
    expect(looks.map((look) => look?.localName)).toEqual(["span", "span", "code"]);
    expect(looks.map((look) => look?.getAttribute("data-math-style"))).toEqual([
      "inline",
      "display",
      "inline",
    ]);
    expect(looks.map((look) => look?.textContent)).toEqual(["E=mc^2", "x^2", "y"]);
    expect(neared).toEqual(looks);
    expect(atoms()[0]?.contentEditable).toBe("false");
    expect(atoms()[0]?.getAttribute("aria-label")).toBe("block: formula");
    core.setDoc(core.parse("Other.\n"));
    expect(undone).toEqual(looks);
  });

  it("an arrow key selects it, the next one moves past it", async () => {
    await load("Lead $x$ tail.\n");
    const at = atomAt();
    const view = core.view;
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, at)));
    key(view.dom, "ArrowRight", { keyCode: 39 });
    expect(view.state.selection).toBeInstanceOf(NodeSelection);
    expect(view.state.selection.from).toBe(at);
    key(view.dom, "ArrowRight", { keyCode: 39 });
    expect(view.state.selection).toBeInstanceOf(TextSelection);
    expect(view.state.selection.from).toBe(at + 1);
  });
});

describe("typing while an inline formula is selected", () => {
  it("the character goes right after it; the formula keeps its bytes", async () => {
    const text = "Lead $E=mc^2$ tail.\n";
    const built = await load(text);
    const view = core.view;
    view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, atomAt())));
    // keydown, then keypress: ProseMirror types a key itself over a node selection, else the browser does at the caret
    const init = { key: "x", bubbles: true, cancelable: true };
    view.dom.dispatchEvent(new KeyboardEvent("keydown", init));
    const pressed = new KeyboardEvent("keypress", { ...init, charCode: 120 });
    view.dom.dispatchEvent(pressed);
    if (!pressed.defaultPrevented) view.dispatch(view.state.tr.insertText("x"));
    expect(sourceAt()).toBe("$E=mc^2$");
    expect(compose(core, built, view.state.doc).text).toBe("Lead $E=mc^2$x tail.\n");
  });
});

describe("an inline formula opened", () => {
  const TEXT = "Lead $E=mc^2$ tail \\$5.\n\nNext words.\n";

  it("a click opens its TeX; typing goes in at once and changes only those bytes", async () => {
    const built = await load(TEXT);
    const field = openAtom();
    expect(field.value).toBe("E=mc^2");
    expect(document.activeElement).toBe(field);
    type(field, "E=mc^3");
    expect(sourceAt()).toBe("$E=mc^3$");
    const composed = compose(core, built, core.view.state.doc);
    expect(composed.text).toBe(TEXT.replace("mc^2", "mc^3"));
    expect(composed.changed).toBe(1);
    // keys in the field are its own: Backspace never deletes the atom
    key(field, "Backspace");
    expect(core.view.state.doc.nodeAt(atomAt())?.type.name).toBe(INLINE_SOURCE);
  });

  it("Escape closes it: the caret after the formula, drawn again from its new TeX", async () => {
    await load(TEXT);
    const field = openAtom();
    type(field, "a+b");
    expect(key(field, "Escape").defaultPrevented).toBe(true);
    const atom = atoms()[0];
    expect(atom?.querySelector("input")).toBeNull();
    const look = atom?.querySelector("[data-math-style]");
    expect(look?.textContent).toBe("a+b");
    expect(neared.at(-1)).toBe(look);
    const selection = core.view.state.selection;
    expect(selection).toBeInstanceOf(TextSelection);
    expect(selection.from).toBe(atomAt() + 1);
  });

  it("Enter on a selected formula opens it; focus moving elsewhere closes it", async () => {
    await load(TEXT);
    const view = core.view;
    view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, atomAt())));
    key(view.dom, "Enter");
    const field = atoms()[0]?.querySelector("input");
    expect(field?.value).toBe("E=mc^2");
    expect(sourceAt()).toBe("$E=mc^2$");
    field?.blur();
    expect(atoms()[0]?.querySelector("input")).toBeNull();
  });

  it("an emptied formula is removed on close; one undo brings it back byte for byte", async () => {
    const built = await load(TEXT);
    const field = openAtom();
    type(field, "x");
    type(field, " ");
    key(field, "Escape");
    expect(atoms()).toHaveLength(0);
    expect(compose(core, built, core.view.state.doc).text).toBe(
      "Lead  tail \\$5.\n\nNext words.\n",
    );
    undo(core.view.state, core.view.dispatch);
    const back = compose(core, built, core.view.state.doc);
    expect(back.text).toBe(TEXT);
    expect(back.changed).toBe(0);
  });

  it("TeX that would not read as one formula is not put in, and the field says so", async () => {
    await load(TEXT);
    const field = openAtom();
    type(field, "a $ b");
    expect(sourceAt()).toBe("$E=mc^2$");
    expect(field.getAttribute("aria-invalid")).toBe("true");
    // spaces at the edges are left out
    type(field, " a+b ");
    expect(sourceAt()).toBe("$a+b$");
    expect(field.hasAttribute("aria-invalid")).toBe(false);
    expect(field.value).toBe(" a+b ");
  });

  it("Ctrl+Z in the field is the editor's undo", async () => {
    await load(TEXT);
    const field = openAtom();
    type(field, "E=mc^3");
    expect(key(field, "z", { ctrlKey: true }).defaultPrevented).toBe(true);
    expect(sourceAt()).toBe("$E=mc^2$");
    expect(field.value).toBe("E=mc^2");
  });

  it("an IME composition goes in only once it is chosen", async () => {
    await load(TEXT);
    const field = openAtom();
    field.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    type(field, "E=か", { isComposing: true });
    expect(sourceAt()).toBe("$E=mc^2$");
    key(field, "Escape", { isComposing: true });
    expect(atoms()[0]?.querySelector("input")).toBe(field);
    type(field, "E=化", { isComposing: true });
    field.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "化" }));
    expect(sourceAt()).toBe("$E=化$");
  });

  it("an arrow key out of its start | end puts the caret before | after it", async () => {
    await load(TEXT);
    let field = openAtom();
    field.setSelectionRange(0, 0);
    expect(key(field, "ArrowLeft").defaultPrevented).toBe(true);
    expect(core.view.state.selection.from).toBe(atomAt());
    field = openAtom();
    field.setSelectionRange(2, 2);
    expect(key(field, "ArrowRight").defaultPrevented).toBe(false);
    field.setSelectionRange(field.value.length, field.value.length);
    key(field, "ArrowRight");
    expect(atoms()[0]?.querySelector("input")).toBeNull();
    expect(core.view.state.selection.from).toBe(atomAt() + 1);
  });

  it("display and code math keep their own delimiters", async () => {
    const text = "A $$x^2$$ and $`y`$ b.\n";
    const built = await load(text);
    const first = openAtom(0);
    type(first, "x^3");
    key(first, "Escape");
    const second = openAtom(1);
    expect(second.value).toBe("y");
    type(second, "z");
    expect(compose(core, built, core.view.state.doc).text).toBe("A $$x^3$$ and $`z`$ b.\n");
  });
});

describe("a block that keeps its math as written", () => {
  it("multi-line inline math stays a locked block", async () => {
    await load("Energy $E=\nmc^2$ here.\n");
    expect(core.view.state.doc.child(0).type.name).toBe(LOCKED);
  });
});

describe("footnote references", () => {
  const NOTES = "Lead[^b] mid[^a] and again[^b] end.\n\n[^a]: Note A.\n\n[^b]: Note B.\n";

  /** The numbers the footnote pieces show, in order. */
  const numbers = (): (string | null | undefined)[] =>
    atoms().map(
      (atom) => atom.querySelector("sup.footnote-ref > a[data-footnote-ref]")?.textContent,
    );

  it("a paragraph using footnotes is editable text, each reference a piece kept as written", async () => {
    const built = await load(NOTES);
    expect(built.sources.map((s) => s.locked)).toEqual([null, "definition", "definition"]);
    expect(core.view.state.doc.child(0).type.name).toBe("paragraph");
    expect(atoms().map((atom) => atom.dataset.kind)).toEqual(["footnote", "footnote", "footnote"]);
    expect([0, 1, 2].map((n) => sourceAt(n))).toEqual(["[^b]", "[^a]", "[^b]"]);
    const composed = compose(core, built, core.view.state.doc);
    expect(composed.text).toBe(NOTES);
    expect(composed.changed).toBe(0);
  });

  it("is drawn as View draws it: a superscript number by first reference", async () => {
    await load(NOTES);
    expect(numbers()).toEqual(["1", "2", "1"]);
    const links = atoms().map((atom) => atom.querySelector("a")?.getAttribute("href"));
    expect(links).toEqual(["#fn-b", "#fn-a", "#fn-b"]);
    expect(atoms()[0]?.getAttribute("aria-label")).toBe("block: footnote");
    expect(atoms()[0]?.contentEditable).toBe("false");
  });

  it("numbers follow when a reference goes and comes back", async () => {
    await load(NOTES);
    const at = atomAt(0);
    core.view.dispatch(core.view.state.tr.delete(at, at + 1));
    expect(numbers()).toEqual(["1", "2"]);
    undo(core.view.state, core.view.dispatch);
    expect(numbers()).toEqual(["1", "2", "1"]);
  });

  it("counts references in blocks kept as written too, in document order", async () => {
    await load(
      "A [long\nname][s] first[^b]\n\nThen[^a] and[^b].\n\n[^a]: A.\n\n[^b]: B.\n\n[s]: https://example.com\n",
    );
    expect(core.view.state.doc.child(0).type.name).toBe(LOCKED);
    expect(numbers()).toEqual(["2", "1"]);
  });

  it("a label matches as the parser matches it (case aside); a reference to no footnote stays text", async () => {
    await load("One[^Note] two[^none] end.\n\n[^note]: N.\n");
    expect(core.view.state.doc.child(0).type.name).toBe("paragraph");
    expect(atoms().map((_, n) => sourceAt(n))).toEqual(["[^Note]"]);
    expect(core.view.state.doc.child(0).textContent).toContain("two[^none] end.");
  });

  it("a click opens its source: another defined label goes in, an undefined one is refused", async () => {
    const built = await load(NOTES);
    const field = openAtom(1);
    expect(field.value).toBe("[^a]");
    type(field, "[^b]");
    expect(sourceAt(1)).toBe("[^b]");
    type(field, "[^zz]");
    expect(sourceAt(1)).toBe("[^b]");
    expect(field.getAttribute("aria-invalid")).toBe("true");
    key(field, "Escape");
    expect(numbers()).toEqual(["1", "1", "1"]);
    expect(compose(core, built, core.view.state.doc).text).toBe(
      NOTES.replace("mid[^a]", "mid[^b]"),
    );
  });

  it("Ctrl+click follows its in-page anchor (nothing in Edit) and opens nothing", async () => {
    await load(NOTES);
    const atom = atoms()[0];
    const anchor = atom?.querySelector("a");
    if (!anchor) throw new Error("no anchor");
    press(anchor, "click", { ctrlKey: true });
    expect(followed).toEqual(["#fn-b"]);
    expect(atom?.querySelector("input")).toBeNull();
  });
});

describe("reference-style links", () => {
  const LINKS =
    'See [the guide][g], [Docs][] and [docs] here.\n\n[g]: https://example.com/guide "Guide"\n[docs]: docs/index.md\n';

  const anchors = (): (HTMLAnchorElement | null | undefined)[] =>
    atoms().map((atom) => atom.querySelector("a"));

  it("a paragraph using them is editable text, each link a piece kept as written", async () => {
    const built = await load(LINKS);
    expect(built.sources.map((s) => s.locked)).toEqual([null, "definition"]);
    expect(atoms().map((atom) => atom.dataset.kind)).toEqual(["link", "link", "link"]);
    expect([0, 1, 2].map((n) => sourceAt(n))).toEqual(["[the guide][g]", "[Docs][]", "[docs]"]);
    const composed = compose(core, built, core.view.state.doc);
    expect(composed.text).toBe(LINKS);
    expect(composed.changed).toBe(0);
  });

  it("is drawn as View's link: its text, the href and title of its definition", async () => {
    await load(LINKS);
    expect(anchors().map((a) => a?.textContent)).toEqual(["the guide", "Docs", "docs"]);
    expect(anchors().map((a) => a?.getAttribute("href"))).toEqual([
      "https://example.com/guide",
      "docs/index.md",
      "docs/index.md",
    ]);
    expect(anchors()[0]?.title).toBe("Guide");
    // View's hooks see the link (a missing target is marked as in View)
    expect(neared).toContain(anchors()[1]);
    expect(atoms()[0]?.getAttribute("aria-label")).toBe("block: link");
  });

  it("Ctrl+click | a middle click follows the resolved href; a click opens the source", async () => {
    await load(LINKS);
    const anchor = anchors()[0];
    if (!anchor) throw new Error("no anchor");
    press(anchor, "click", { ctrlKey: true });
    expect(followed).toEqual(["https://example.com/guide"]);
    expect(atoms()[0]?.querySelector("input")).toBeNull();
    // the middle button as Ctrl + click
    press(anchor, "auxclick", { button: 1 });
    const both = ["https://example.com/guide", "https://example.com/guide"];
    expect(followed).toEqual(both);
    const field = openAtom(0);
    expect(field.value).toBe("[the guide][g]");
    type(field, "[the handbook][g]");
    expect(sourceAt(0)).toBe("[the handbook][g]");
    key(field, "Escape");
    expect(anchors()[0]?.textContent).toBe("the handbook");
    expect(followed).toEqual(both);
  });

  it("labels match as the parser matches them: case and inner spaces aside", async () => {
    await load("A [The   Guide] b.\n\n[the guide]: https://example.com/g\n");
    expect(anchors().map((a) => a?.getAttribute("href"))).toEqual(["https://example.com/g"]);
  });

  it("a shortcut with no definition stays plain text, as in View", async () => {
    const text = "A [nothing] here and [g].\n\n[g]: https://example.com/g\n";
    const built = await load(text);
    expect(atoms().map((_, n) => sourceAt(n))).toEqual(["[g]"]);
    expect(core.view.state.doc.child(0).textContent).toContain("A [nothing] here");
    expect(compose(core, built, core.view.state.doc).text).toBe(text);
  });

  it("an unsafe destination is no href", async () => {
    await load("A [x] b.\n\n[x]: javascript:alert(1)\n");
    expect(anchors()[0]?.hasAttribute("href")).toBe(false);
    expect(anchors()[0]?.textContent).toBe("x");
  });
});

describe("HTML written in a line", () => {
  const LINE =
    "Press <kbd>Ctrl</kbd>+<kbd>F</kbd>, H<sub>2</sub>O, x<sup>2</sup>, <mark>this</mark>,<br>next.\n";

  beforeEach(() => {
    viewHtml.clear();
    renders = 0;
  });

  it("a paragraph using it is editable text, each element a piece kept as written", async () => {
    expect(atomsIn(LINE)).toEqual([
      "<kbd>Ctrl</kbd>",
      "<kbd>F</kbd>",
      "<sub>2</sub>",
      "<sup>2</sup>",
      "<mark>this</mark>",
      "<br>",
    ]);
    // an element with Markdown inside is one piece; a tag without its end stays as it is
    expect(atomsIn("A <kbd>**Ctrl**</kbd> and <kbd>open.\n")).toEqual(["<kbd>**Ctrl**</kbd>"]);
    const built = await load(LINE);
    expect(built.sources.map((s) => s.locked)).toEqual([null]);
    expect(atoms().map((atom) => atom.dataset.kind)).toEqual(Array(6).fill("html"));
    const composed = compose(core, built, core.view.state.doc);
    expect(composed.text).toBe(LINE);
    expect(composed.changed).toBe(0);
  });

  it("is drawn as View's element for its source; with none known, its source shows as text", async () => {
    viewHtml.set("<kbd>Ctrl</kbd>", "<kbd>Ctrl</kbd>");
    viewHtml.set("<br>", "<br>");
    await load(LINE.replace("<kbd>F</kbd>", "<kbd>G</kbd>"));
    const [ctrl, g, , , , br] = atoms();
    expect(ctrl?.querySelector("kbd")?.textContent).toBe("Ctrl");
    expect(br?.querySelector("br")).not.toBeNull();
    // never the person's HTML: an element View has not drawn is its source, as text
    expect(g?.querySelector("kbd")).toBeNull();
    expect(g?.textContent).toBe("<kbd>G</kbd>");
    expect(neared).toContain(ctrl?.querySelector("kbd"));
    expect(ctrl?.getAttribute("aria-label")).toBe("block: html");
    // the looks a later render of the document brings are drawn when it comes
    viewHtml.set("<kbd>G</kbd>", "<kbd>G</kbd>");
    relook(core.view);
    expect(atoms()[1]?.querySelector("kbd")?.textContent).toBe("G");
  });

  it("a click opens its source; what is typed goes in as written, drawn from View's look of it", async () => {
    viewHtml.set("<kbd>Ctrl</kbd>", "<kbd>Ctrl</kbd>");
    viewHtml.set("<kbd>Alt</kbd>", "<kbd>Alt</kbd>");
    const built = await load("Press <kbd>Ctrl</kbd> now.\n");
    const field = openAtom(0);
    expect(field.value).toBe("<kbd>Ctrl</kbd>");
    type(field, "<kbd>Alt</kbd>");
    expect(sourceAt(0)).toBe("<kbd>Alt</kbd>");
    key(field, "Escape");
    expect(atoms()[0]?.querySelector("kbd")?.textContent).toBe("Alt");
    expect(compose(core, built, core.view.state.doc).text).toBe("Press <kbd>Alt</kbd> now.\n");
    expect(renders).toBe(0);
    // a source no render has drawn yet: the document is rendered again once typing pauses
    type(openAtom(0), "<kbd>Shift</kbd>");
    await vi.waitFor(() => expect(renders).toBe(1));
  });
});

describe("saving around reference pieces", () => {
  const TEXT =
    "[^n] lead [the site][s] mid [s] tail.\n\nNext words.\n\n[^n]: Note.\n\n[s]: https://example.com\n";

  function typeAt(pos: number, text: string): void {
    core.view.dispatch(core.view.state.tr.insertText(text, pos));
  }

  it.each([
    ["before a link", 1, 0, " [the site]", " Z[the site]"],
    ["after a link", 1, 1, "[s] mid", "[s]Z mid"],
    ["after a shortcut", 2, 1, "[s] tail", "[s]Z tail"],
    ["after a footnote", 0, 1, "[^n] lead", "[^n]Z lead"],
  ])("typing %s changes only the typed character", async (_, n, after, find, by) => {
    const built = await load(TEXT);
    typeAt(atomAt(n) + after, "Z");
    const composed = compose(core, built, core.view.state.doc);
    expect(composed.text).toBe(TEXT.replace(find, () => by));
    expect(composed.changed).toBe(1);
  });

  it.each([
    ["`(` after a shortcut stays text (never a link destination)", 2, "(x)", "[s]\\(x) tail"],
    ["`(` after a footnote, before another piece, stays text", 0, "(x)", "[^n]\\(x) lead"],
    ["`[` after a shortcut stays text (never a full reference)", 2, "[g]", "[s]\\[g] tail"],
    [
      "`:` after a piece opening the paragraph stays text (never a definition)",
      0,
      ":",
      "[^n]\\: lead",
    ],
    ["`:` with more text after it", 0, ": x.", "[^n]\\: x. lead"],
  ])("%s", async (_, n, typed, expected) => {
    const built = await load(TEXT);
    typeAt(atomAt(n) + 1, typed);
    expect(compose(core, built, core.view.state.doc).text).toContain(expected);
  });

  it("deleting a piece drops only its bytes", async () => {
    const built = await load(TEXT);
    const at = atomAt(1);
    core.view.dispatch(core.view.state.tr.delete(at, at + 1));
    expect(compose(core, built, core.view.state.doc).text).toBe(TEXT.replace("[the site][s]", ""));
  });
});

describe("blocks that keep their references as written", () => {
  it.each([
    ["a picture by reference", "An ![logo][s] here.\n\n[s]: logo.png\n", "reference"],
    ["a link across lines", "A [long\nname][s] here.\n\n[s]: https://example.com\n", "reference"],
    ["a definition", "[s]: https://example.com\n\nUse [s].\n", "definition"],
  ])("%s", async (_, text, reason) => {
    const built = await load(text);
    expect(built.sources.map((s) => s.locked)).toContain(reason);
    expect(compose(core, built, core.view.state.doc).text).toBe(text);
  });
});

describe("what is typed next to a piece reads back as typed", () => {
  const DEFS = "\n\n[^n]: Note.\n\n[s]: https://example.com\n\n[docs]: d.md\n";
  const PIECES = ["$x$", "$$x$$", "$`x`$", "[^n]", "[the site][s]", "[Docs][]", "[s]"];

  function typeAt(pos: number, text: string): void {
    core.view.dispatch(core.view.state.tr.insertText(text, pos));
  }

  /** The first block as saved, read again: the same pieces and text the editor shows. */
  function readsBack(built: Built): string {
    const composed = compose(core, built, core.view.state.doc);
    const range = composed.ranges[0];
    if (!range) throw new Error("no block");
    const saved = composed.text.slice(range.from, range.to);
    const again = core.parse(saved);
    expect(again.childCount).toBe(1);
    expect(again.child(0).toJSON()).toEqual(core.view.state.doc.child(0).toJSON());
    expect(composed.lost).toEqual([]);
    return saved;
  }

  it("a digit typed right after a formula is written as a character reference", async () => {
    const built = await load("Cost $x$ now.\n");
    typeAt(atomAt() + 1, "5");
    expect(readsBack(built)).toBe("Cost $x$&#53; now.");
  });

  it("a space deleted between a formula and a digit keeps the formula", async () => {
    const built = await load("Sum $a$ 5 more.\n");
    const at = atomAt() + 1;
    core.view.dispatch(core.view.state.tr.delete(at, at + 1));
    expect(readsBack(built)).toBe("Sum $a$&#53; more.");
  });

  it("a digit after display or code math stays as typed (no rule against it there)", async () => {
    const built = await load("A $$x$$ and $`y`$ b.\n");
    typeAt(atomAt(1) + 1, "5");
    typeAt(atomAt(0) + 1, "5");
    expect(readsBack(built)).toBe("A $$x$$5 and $`y`$5 b.");
  });

  it.each(
    PIECES.flatMap((piece) => ["$", "5", "a", "\\", "]", "(", "[", ":"].map((c) => [piece, c])),
  )("%s then %s", async (piece, typed) => {
    const built = await load(`${piece} tail.${DEFS}`);
    typeAt(atomAt() + 1, typed);
    readsBack(built);
  });

  it.each(PIECES.flatMap((piece) => ["$", "5", "\\", "!", "[", "]"].map((c) => [c, piece])))(
    "%s then %s",
    async (typed, piece) => {
      const built = await load(`Lead ${piece}${DEFS}`);
      typeAt(atomAt(), typed);
      readsBack(built);
    },
  );

  it("a `:` after a bracket typed as prose is not escaped", async () => {
    const built = await load("Note.\n");
    typeAt(1, "[12:30]: ok ");
    expect(readsBack(built)).toBe("\\[12:30]: ok Note.");
  });

  // a text ending in a space right before a piece | mark is its own text node: its `[` must stay escaped, or `[s]`
  // reads as a link wherever `s` is defined
  const NEXT: [string, string][] = [
    ["a formula", "$x$"],
    ["a reference", "[the site][s]"],
    ["an emphasis", "*em*"],
    ["a link", "[site](https://example.com)"],
  ];
  it.each(
    NEXT.flatMap(([name, next]) => {
      const cases: [string, string, string, string][] = [[name, next, "defined", DEFS]];
      // without its definition a reference is text: no piece to type before
      if (next !== "[the site][s]") cases.push([name, next, "not defined", "\n"]);
      return cases;
    }),
  )("`[s] ` typed before %s (`%s`), `s` %s, keeps its escape", async (_, next, __, defs) => {
    const built = await load(`Lead ${next} tail.${defs}`);
    typeAt("Lead ".length + 1, "[s] ");
    expect(readsBack(built)).toBe(`Lead \\[s] ${next} tail.`);
  });

  it.each(NEXT)(
    "a block holding `\\[s] ` before %s opens editable and keeps it when edited elsewhere",
    async (_, next) => {
      const built = await load(`Lead \\[s] ${next} tail.${DEFS}`);
      expect(built.sources[0]?.locked).toBeNull();
      const end = core.view.state.doc.child(0).nodeSize - 1;
      typeAt(end, " More.");
      expect(readsBack(built)).toBe(`Lead \\[s] ${next} tail. More.`);
    },
  );

  it.each([
    ["# ", "\\# "],
    ["1. ", "1\\. "],
    ["- ", "\\- "],
  ])("`%s` typed at the start of a text ending in a space stays text", async (typed, written) => {
    const built = await load("Lead *em* tail.\n");
    typeAt(1, typed);
    expect(readsBack(built)).toBe(`${written}Lead *em* tail.`);
  });
});

describe("pieces that do not read back", () => {
  it("two references joined by deleting the space between them read as one link: the save says so", async () => {
    const built = await load("See [a] [b] here.\n\n[a]: a.md\n\n[b]: b.md\n");
    const at = atomAt(0) + 1;
    core.view.dispatch(core.view.state.tr.delete(at, at + 1));
    const composed = compose(core, built, core.view.state.doc);
    expect(composed.text).toContain("See [a][b] here.");
    // Markdown reads `[a][b]` as one full reference: text `a`, label `b`
    expect(atomsIn("See [a][b] here.")).toEqual(["[a][b]"]);
    expect(composed.lost).toEqual([0]);
  });

  it("a piece whose source no longer reads as one is reported, its block written by the editor", async () => {
    const built = await load("Lead $x$ tail.\n\nNext.\n");
    const at = atomAt();
    const node = core.view.state.doc.nodeAt(at);
    if (!node) throw new Error("no atom");
    core.view.dispatch(
      core.view.state.tr.setNodeMarkup(at, undefined, { ...node.attrs, source: "$ x$" }),
    );
    const composed = compose(core, built, core.view.state.doc);
    expect(composed.text).toBe("Lead $ x$ tail.\n\nNext.\n");
    expect(composed.lost).toEqual([0]);
  });
});
