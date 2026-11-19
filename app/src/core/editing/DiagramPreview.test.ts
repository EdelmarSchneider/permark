// @vitest-environment jsdom
// The diagram beside Source is a picture of the block being typed: a link drawn in it is never followed (the web view
// would load it in place of the app page and drop the unsaved text), nor reached by Tab. A click anywhere in it does
// nothing: the picture has no click behavior to keep.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NO_REQUIREMENT } from "../conditions/available";
import { contributions } from "../plugins/instance.svelte";
import DiagramPreview from "./DiagramPreview.svelte";

let component: Record<string, unknown> | null = null;
let remove: (() => void) | null = null;
let target: HTMLElement;

beforeEach(() => {
  remove = contributions.register({
    owner: "core",
    requires: NO_REQUIREMENT,
    kind: "embed",
    id: "test.mermaid",
    language: "mermaid",
    view: () => import("../testing/DiagramProbe.svelte"),
  });
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  remove?.();
  remove = null;
  document.body.replaceChildren();
});

/** The preview drawn for `source` (after its pause for typing), with its link and a shape outside the link. */
async function drawn(source: string): Promise<{ link: Element; shape: Element }> {
  component = mount(DiagramPreview, { target, props: { language: "mermaid", source } });
  flushSync();
  await vi.waitFor(() => expect(target.querySelector("svg a text")).not.toBeNull(), {
    timeout: 5_000,
    interval: 10,
  });
  const link = target.querySelector("svg a text");
  const shape = target.querySelector("svg > rect");
  if (link === null || shape === null) throw new Error("no drawing");
  return { link, shape };
}

function press(on: Element, type: "click" | "auxclick", button: number): MouseEvent {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, button });
  on.dispatchEvent(event);
  return event;
}

describe("links in the diagram beside Source", () => {
  it("are not followed by a click", async () => {
    const { link } = await drawn("link");
    expect(press(link, "click", 0).defaultPrevented).toBe(true);
  });

  it("are not opened elsewhere by the middle button", async () => {
    const { link } = await drawn("link");
    expect(press(link, "auxclick", 1).defaultPrevented).toBe(true);
  });

  it("are not followed through a shape either: every click in the picture is held", async () => {
    // a <use> copy of a link takes the click as itself, outside any link the guard could look for
    const { shape } = await drawn("link");
    expect(press(shape, "click", 0).defaultPrevented).toBe(true);
    expect(press(shape, "auxclick", 1).defaultPrevented).toBe(true);
  });

  it("are out of the tab order (the drawing is inert)", async () => {
    const { link } = await drawn("link");
    const wrapper = link.closest("[inert]");
    expect(wrapper).not.toBeNull();
    // the whole drawing sits under the inert wrapper, so a redraw keeps its links out of reach of Tab
    expect(wrapper?.contains(target.querySelector("svg"))).toBe(true);
  });
});
