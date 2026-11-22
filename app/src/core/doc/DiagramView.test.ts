// @vitest-environment jsdom
// A <use> in a drawing resolves its `#id` against the whole page, so it can copy a link of another drawing; a click on
// that copy is retargeted to the <use> and would pass every link router. Such a click is held; a real link is not.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { declarations, ruleBody, styleOf } from "../testing/cssRules";
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

describe("a diagram that cannot be drawn", () => {
  async function failed(source: string): Promise<HTMLElement> {
    const target = document.createElement("div");
    document.body.append(target);
    component = mount(DiagramView, {
      target,
      props: {
        source,
        theme: "light",
        draw: () => Promise.resolve({ problem: "This diagram is too large to draw here." }),
      },
    });
    flushSync();
    await vi.waitFor(() => expect(target.querySelector(".is-failed")).not.toBeNull());
    return target;
  }

  it("says why first, then shows its source in a box of its own the keyboard can scroll", async () => {
    const target = await failed(`@startuml\n${"a -> b\n".repeat(3_200)}@enduml`);
    const figure = target.querySelector("figure");
    expect(figure?.firstElementChild?.matches("figcaption.message")).toBe(true);
    expect(figure?.querySelector(".problem")?.textContent).toBe(
      "This diagram is too large to draw here.",
    );
    const source = figure?.querySelector("pre.source");
    expect(source?.getAttribute("tabindex")).toBe("0");
    expect(source?.getAttribute("role")).toBe("region");
    expect(source?.getAttribute("aria-label")).toBe("Diagram source");
  });

  it("caps the source's height: a long one scrolls inside and never pushes the page for screens", () => {
    const style = styleOf(
      readFileSync(join(process.cwd(), "src/core/doc/DiagramView.svelte"), "utf8"),
    );
    const rules = declarations(ruleBody(style, ".source"));
    expect(rules.some((rule) => rule.startsWith("max-height:"))).toBe(true);
    expect(rules).toContain("overflow: auto");
  });
});
