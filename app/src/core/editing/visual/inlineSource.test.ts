// @vitest-environment jsdom
import "./jsdomShims";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { undo } from "@milkdown/kit/prose/history";
import { NodeSelection, TextSelection } from "@milkdown/kit/prose/state";
import type { Segment } from "../../../api/generated/Segment";
import { classify } from "./classify";
import { buildDoc, compose, type Built } from "./compose";
import { createCore, INLINE_SOURCE, LOCKED, type Core, type LockedHost } from "./core";
import { appPlugins, type PluginHost } from "./plugins";

let core: Core;
const neared: HTMLElement[] = [];
const undone: HTMLElement[] = [];

const host: LockedHost = {
  render: () => null,
  refresh: () => Promise.resolve(),
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
  followLink: () => {},
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

function press(target: Element, type: "mousedown" | "click", init: MouseEventInit = {}): void {
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
    ['Html <span title="$x$">t</span> $y$', ["$y$"]],
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
