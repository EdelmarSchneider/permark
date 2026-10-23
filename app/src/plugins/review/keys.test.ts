// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { overlayShown } from "../../core/doc/escape";
import { closesReview, type EscapeEnv, type EscapeEvent } from "./keys";

const env = (over: Partial<EscapeEnv> = {}): EscapeEnv => ({
  reviewing: true,
  editing: false,
  modal: false,
  zoomed: false,
  finding: false,
  overlay: false,
  ...over,
});

const key = (over: Partial<EscapeEvent> = {}): EscapeEvent => ({
  key: "Escape",
  isComposing: false,
  defaultPrevented: false,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  target: document.body,
  ...over,
});

function inside(html: string, selector: string): Element {
  document.body.innerHTML = html;
  const el = document.querySelector(selector);
  if (el === null) throw new Error(selector);
  return el;
}

describe("Esc closes the review", () => {
  it("from the page, a review bar button or a change's Keep", () => {
    expect(closesReview(key(), env())).toBe(true);
    const radio = inside('<div role="radiogroup"><button role="radio">x</button></div>', "button");
    expect(closesReview(key({ target: radio }), env())).toBe(true);
    const keep = inside('<div class="rv-bar"><button>Keep</button></div>', "button");
    expect(closesReview(key({ target: keep }), env())).toBe(true);
  });

  it("only Esc, plain, not while composing, not when someone took it", () => {
    expect(closesReview(key({ key: "Enter" }), env())).toBe(false);
    expect(closesReview(key({ isComposing: true }), env())).toBe(false);
    expect(closesReview(key({ defaultPrevented: true }), env())).toBe(false);
    for (const mod of ["ctrlKey", "metaKey", "altKey", "shiftKey"] as const)
      expect(closesReview(key({ [mod]: true }), env())).toBe(false);
  });

  it("not when there is no review", () => {
    expect(closesReview(key(), env({ reviewing: false }))).toBe(false);
  });

  it("innermost first: a field, a menu, a dialog keep their Esc", () => {
    for (const [html, sel] of [
      ["<input>", "input"],
      ["<textarea></textarea>", "textarea"],
      ["<select></select>", "select"],
      ['<div role="menu"><button>a</button></div>', "button"],
      ['<div role="dialog"><button>a</button></div>', "button"],
      ['<div role="listbox"><div role="option">a</div></div>', "[role=option]"],
    ] as const) {
      expect(closesReview(key({ target: inside(html, sel) }), env())).toBe(false);
    }
  });

  it("an editor, the find bar, the zoom, an overlay or a modal box go first", () => {
    for (const k of ["editing", "modal", "zoomed", "finding", "overlay"] as const)
      expect(closesReview(key(), env({ [k]: true }))).toBe(false);
  });
});

describe("an overlay is shown", () => {
  it("a dialog | menu in the page counts, a hidden one or a closed <dialog> does not", () => {
    document.body.innerHTML = "<main></main>";
    expect(overlayShown()).toBe(false);
    document.body.innerHTML = '<div role="dialog">ask</div>';
    expect(overlayShown()).toBe(true);
    document.body.innerHTML = '<div hidden><div role="menu"></div></div>';
    expect(overlayShown()).toBe(false);
    document.body.innerHTML = "<dialog></dialog>";
    expect(overlayShown()).toBe(false);
    document.body.innerHTML = "<dialog open></dialog>";
    expect(overlayShown()).toBe(true);
  });
});
