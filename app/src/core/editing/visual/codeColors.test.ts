// @vitest-environment jsdom
import "./jsdomShims";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { undo } from "@milkdown/kit/prose/history";
import { TextSelection } from "@milkdown/kit/prose/state";
import type { CodeColorer, CodeSpan } from "../../plugins/props";
import { COLORS_DELAY_MS } from "./codeColors";
import { createCore, type CodeHost, type Core } from "./core";

let core: Core;
/** The texts the colorer was asked for, in order. */
let asked: string[] = [];
/** null: the host offers no colors (the plugin that brings them is off). */
let colorer: CodeColorer | null = null;

/** Keywords and numbers, as a highlighter would mark them. */
const fakeColorer: CodeColorer = (source) => {
  asked.push(source);
  const spans: CodeSpan[] = [];
  for (const match of source.matchAll(/\b(let|const)\b|\b\d+\b/g)) {
    const classes = [match[1] === undefined ? "hljs-number" : "hljs-keyword"];
    spans.push({ from: match.index, to: match.index + match[0].length, classes });
  }
  return Promise.resolve(spans);
};

const host: CodeHost = {
  label: () => null,
  colors: () => (colorer === null ? null : Promise.resolve(colorer)),
};

beforeAll(async () => {
  const target = document.createElement("div");
  document.body.append(target);
  core = await createCore({ host: target, code: host });
});

afterAll(async () => {
  await core.destroy();
});

beforeEach(() => {
  vi.useFakeTimers();
  asked = [];
  colorer = fakeColorer;
});

afterEach(() => {
  vi.useRealTimers();
});

const markdown = (): string => core.serialize(core.view.state.doc).trim();

function load(text: string): void {
  core.setDoc(core.parse(text));
}

/** Lets the delay pass and the colorer answer. */
async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(COLORS_DELAY_MS + 10);
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

function typeAfter(text: string, typed: string): void {
  const view = core.view;
  const at = after(text);
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, at)));
  view.dispatch(view.state.tr.insertText(typed));
}

const blocks = (): HTMLElement[] =>
  Array.from(core.view.dom.querySelectorAll<HTMLElement>(":scope > .code-block"));

const tokens = (block: HTMLElement | undefined, className: string): string[] =>
  Array.from(block?.querySelectorAll(`code .${className}`) ?? []).map((e) => e.textContent);

describe("code colors in Edit", () => {
  it("paints the colorer's spans in the code block, the text and the Markdown unchanged", async () => {
    const text = "Intro\n\n```js\nlet a = 1;\n```\n";
    load(text);
    const before = core.view.state.doc;
    await settle();
    const [block] = blocks();
    expect(tokens(block, "hljs-keyword")).toEqual(["let"]);
    expect(tokens(block, "hljs-number")).toEqual(["1"]);
    expect(block?.querySelector("pre > code")?.textContent).toBe("let a = 1;");
    // colors are drawn, not written: the document is the same node, its Markdown the same text
    expect(core.view.state.doc).toBe(before);
    expect(markdown()).toBe(text.trim());
  });

  it("colors follow typing after the delay, asking again only for the block that changed", async () => {
    load("```js\nlet a = 1;\n```\n\n```js\nconst b = 2;\n```\n");
    await settle();
    expect(asked).toEqual(["let a = 1;", "const b = 2;"]);
    asked = [];
    typeAfter("const b = 2", "0; let c = 3");
    // while typing: the old colors stay where they were, nothing is asked yet
    await vi.advanceTimersByTimeAsync(COLORS_DELAY_MS / 2);
    expect(asked).toEqual([]);
    expect(tokens(blocks()[1], "hljs-keyword")).toEqual(["const"]);
    await settle();
    expect(asked).toEqual(["const b = 20; let c = 3;"]);
    expect(tokens(blocks()[1], "hljs-keyword")).toEqual(["const", "let"]);
    expect(tokens(blocks()[1], "hljs-number")).toEqual(["20", "3"]);
    expect(tokens(blocks()[0], "hljs-keyword")).toEqual(["let"]);
  });

  it("keeps the caret and the selection where they are when colors arrive", async () => {
    load("```js\nlet a = 1;\n```\n");
    await settle();
    typeAfter("let a", "bc");
    const selection = core.view.state.selection;
    await settle();
    expect(asked.at(-1)).toBe("let abc = 1;");
    expect(core.view.state.selection.eq(selection)).toBe(true);
  });

  it("paints again after an undo, the text back as it was", async () => {
    load("```js\nlet a = 1;\n```\n");
    await settle();
    typeAfter("let a = 1", "2");
    await settle();
    expect(tokens(blocks()[0], "hljs-number")).toEqual(["12"]);
    undo(core.view.state, core.view.dispatch);
    await settle();
    expect(markdown()).toBe("```js\nlet a = 1;\n```");
    expect(tokens(blocks()[0], "hljs-number")).toEqual(["1"]);
  });

  it("waits while a composition is open", async () => {
    load("```js\nlet a = 1;\n```\n");
    await settle();
    asked = [];
    let composing = true;
    Object.defineProperty(core.view, "composing", { configurable: true, get: () => composing });
    try {
      typeAfter("let a", "x");
      await settle();
      expect(asked).toEqual([]);
      composing = false;
      await settle();
      expect(asked).toEqual(["let ax = 1;"]);
    } finally {
      Reflect.deleteProperty(core.view, "composing");
    }
  });

  it("draws code plain when no colors are on offer", async () => {
    colorer = null;
    load("```js\nlet a = 1;\n```\n");
    await settle();
    expect(core.view.dom.querySelector("code [class^='hljs-']")).toBeNull();
    expect(asked).toEqual([]);
  });

  it("takes the colors away from a block that changes once none are on offer", async () => {
    load("```js\nlet a = 1;\n```\n");
    await settle();
    expect(tokens(blocks()[0], "hljs-keyword")).toEqual(["let"]);
    colorer = null;
    typeAfter("let a = 1", "0");
    await settle();
    expect(core.view.dom.querySelector("code [class^='hljs-']")).toBeNull();
  });
});
