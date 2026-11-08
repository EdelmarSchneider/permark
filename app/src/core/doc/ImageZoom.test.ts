// @vitest-environment jsdom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import ImageZoom from "./ImageZoom.svelte";

let component: Record<string, unknown> | null = null;

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
});

function key(on: Element, name: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key: name,
    bubbles: true,
    cancelable: true,
    ...init,
  });
  on.dispatchEvent(event);
  return event;
}

describe("a picture shown large", () => {
  it("takes the keyboard; Tab | Shift+Tab never leave it for the page behind; Escape closes it", () => {
    const target = document.createElement("div");
    document.body.append(target);
    let closed = 0;
    component = mount(ImageZoom, {
      target,
      props: { src: "x.png", alt: "", close: () => void (closed += 1) },
    });
    flushSync();
    const layer = target.querySelector<HTMLElement>("[aria-modal=true]");
    if (!layer) throw new Error("no layer");
    expect(document.activeElement).toBe(layer);
    expect(key(layer, "Tab").defaultPrevented).toBe(true);
    expect(key(layer, "Tab", { shiftKey: true }).defaultPrevented).toBe(true);
    expect(closed).toBe(0);
    key(layer, "Escape");
    expect(closed).toBe(1);
  });
});
