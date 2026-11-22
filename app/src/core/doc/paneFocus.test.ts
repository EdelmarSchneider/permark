// @vitest-environment jsdom
// The document pane draws its focus ring only while it holds a keyboard sent to it on purpose; a keyboard that fell
// back on it (what held it went from under it) draws none.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SENT, sendToPane } from "./paneFocus";

/** A source file of the app (the tests run from its folder). */
const source = (path: string): string => readFileSync(resolve(process.cwd(), "src", path), "utf8");

function pane(): HTMLElement {
  const element = document.createElement("div");
  element.dataset["scroller"] = "";
  element.tabIndex = -1;
  document.body.append(element);
  return element;
}

afterEach(() => {
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

/** Whether the window has the system's keyboard (jsdom never says it has). */
function windowFocused(on: boolean): void {
  vi.spyOn(document, "hasFocus").mockReturnValue(on);
}

describe("the keyboard sent to the document pane", () => {
  it("takes the keyboard and is marked as sent until it loses it", () => {
    windowFocused(true);
    const scroller = pane();
    const other = document.createElement("button");
    document.body.append(other);
    sendToPane(scroller);
    expect(document.activeElement).toBe(scroller);
    expect(scroller.hasAttribute(SENT)).toBe(true);
    other.focus();
    expect(scroller.hasAttribute(SENT)).toBe(false);
    // focused again by anything else (a review closed, a comment saved): not sent, no mark
    scroller.focus();
    expect(scroller.hasAttribute(SENT)).toBe(false);
  });

  it("the window going to the background keeps the mark: back, the pane still shows it holds the keyboard", () => {
    const scroller = pane();
    sendToPane(scroller);
    windowFocused(false);
    scroller.dispatchEvent(new FocusEvent("blur"));
    expect(scroller.hasAttribute(SENT)).toBe(true);
  });

  it("the pane's ring needs the mark: a keyboard that fell back on the page draws no ring there", () => {
    const area = source("core/doc/DocumentArea.svelte");
    const css = area.slice(area.indexOf("<style>"));
    expect(css).toMatch(/\.scroller:focus\s*\{\s*outline:\s*none;/);
    expect(css).toContain(`.scroller:global([${SENT}]):focus-visible {`);
    expect(css).not.toMatch(/\.scroller:focus-visible\s*\{/);
  });
});
