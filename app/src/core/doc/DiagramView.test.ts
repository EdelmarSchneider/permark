// @vitest-environment jsdom
// A <use> in a drawing resolves its `#id` against the whole page, so it can copy a link of another drawing; a click on
// that copy is retargeted to the <use> and would pass every link router. Such a click is held; a real link is not.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import DiagramView from "./DiagramView.svelte";

let component: Record<string, unknown> | null = null;

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
});

const SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><g id="part"><rect width="2" height="2"/></g>' +
  '<use href="#part"/><a href="other.md"><use href="#part"/><text>go</text></a><rect width="10" height="1"/></svg>';

async function drawn(): Promise<HTMLElement> {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(DiagramView, {
    target,
    props: { source: "x", theme: "light", draw: () => Promise.resolve(SVG) },
  });
  flushSync();
  await vi.waitFor(() => expect(target.querySelector("svg")).not.toBeNull());
  return target;
}

function press(on: Element, type: "click" | "auxclick", button: number): MouseEvent {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, button });
  on.dispatchEvent(event);
  return event;
}

describe("a diagram on the page", () => {
  it("holds a click on a copied part outside any link", async () => {
    const target = await drawn();
    const copy = target.querySelector("svg > use");
    if (!copy) throw new Error("no copy");
    expect(press(copy, "click", 0).defaultPrevented).toBe(true);
    expect(press(copy, "auxclick", 1).defaultPrevented).toBe(true);
  });

  it("leaves a click inside a real link, and on a plain shape, to the page", async () => {
    const target = await drawn();
    const inLink = target.querySelector("svg a use");
    const shape = target.querySelector("svg > rect");
    if (!inLink || !shape) throw new Error("no drawing");
    expect(press(inLink, "click", 0).defaultPrevented).toBe(false);
    expect(press(shape, "click", 0).defaultPrevented).toBe(false);
  });
});
