// @vitest-environment jsdom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import Segments from "./Segments.svelte";

type V = "a" | "b" | "c";
const options = [
  { value: "a" as const, label: "A" },
  { value: "b" as const, label: "B" },
  { value: "c" as const, label: "C" },
];

let component: ReturnType<typeof mount> | null = null;
const target = document.body.appendChild(document.createElement("div"));

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.innerHTML = "";
});

function setup(disabledValues: readonly V[] = []): { chosen: V[]; radios: () => HTMLElement[] } {
  const chosen: V[] = [];
  component = mount(Segments<V>, {
    target,
    props: {
      options,
      value: "a",
      label: "Pick",
      disabledValues,
      onchange: (v: V) => chosen.push(v),
    },
  });
  flushSync();
  return {
    chosen,
    radios: () => Array.from(target.querySelectorAll<HTMLElement>('[role="radio"]')),
  };
}

function key(el: HTMLElement, key: string, mods: KeyboardEventInit = {}): KeyboardEvent {
  const e = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...mods });
  el.dispatchEvent(e);
  return e;
}

describe("segments", () => {
  it("only a plain arrow moves the choice: focus, Tab and modified arrows (Alt+Left = back) never do", () => {
    const { chosen, radios } = setup();
    const first = radios()[0]!;
    first.focus();
    key(first, "Tab");
    for (const mods of [
      { altKey: true },
      { ctrlKey: true },
      { metaKey: true },
      { shiftKey: true },
    ]) {
      const e = key(first, "ArrowLeft", mods);
      expect(e.defaultPrevented).toBe(false);
    }
    expect(chosen).toEqual([]);
    key(first, "ArrowRight");
    key(first, "ArrowLeft");
    expect(chosen).toEqual(["b", "c"]);
  });

  it("arrows skip the choices that are off; nowhere to go = no change", () => {
    const { chosen, radios } = setup(["b", "c"]);
    key(radios()[0]!, "ArrowRight");
    key(radios()[0]!, "ArrowDown");
    expect(chosen).toEqual([]);
  });
});
