// @vitest-environment jsdom
import "../testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import OpenManyAsk from "./OpenManyAsk.svelte";

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
});

const buttons = (): HTMLButtonElement[] => [...target.querySelectorAll("button")];

describe("the question before many dropped items open", () => {
  it("Cancel first, Open them last (the order of every dialog); Open them takes the keyboard", async () => {
    const confirm = vi.fn();
    const cancel = vi.fn();
    component = mount(OpenManyAsk, {
      target,
      props: { at: { x: 100, y: 120 }, count: 12, confirm, cancel },
    });
    flushSync();
    await tick();
    flushSync();
    expect(target.querySelector("h2")?.textContent).toBe("Open 12 items as tabs?");
    expect(buttons().map((b) => b.textContent?.trim())).toEqual(["Cancel", "Open them"]);
    expect(document.activeElement).toBe(buttons()[1]);
    buttons()[1]?.click();
    expect(confirm).toHaveBeenCalledOnce();
    buttons()[0]?.click();
    expect(cancel).toHaveBeenCalledOnce();
  });
});
