// @vitest-environment jsdom
// The emoji | symbols popup of the Edit toolbar: the search takes the keyboard when it opens, also when the toolbar
// button that opened it holds the keyboard (a click focuses a button; `autofocus` alone acts only from the page body).
import "../../testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { windowStore } from "../../window.svelte";
import CharacterPicker from "./CharacterPicker.svelte";

let component: Record<string, unknown> | null = null;

beforeEach(() => {
  vi.spyOn(windowStore, "refreshCharacters").mockImplementation(() => {});
  windowStore.characters = [{ name: "greek", entries: [{ char: "α", names: ["alpha"] }] }];
});

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
  windowStore.characters = [];
  vi.restoreAllMocks();
});

describe("the character popup", () => {
  it("puts the keyboard in its search when it opens, from the button that opened it", async () => {
    const opener = document.body.appendChild(document.createElement("button"));
    opener.focus();
    const target = document.body.appendChild(document.createElement("div"));
    component = mount(CharacterPicker, {
      target,
      props: { kind: "symbols", apply: () => {}, close: () => {}, popoverKey: () => {} },
    });
    flushSync();
    await tick();
    expect(document.activeElement).toBe(target.querySelector('input[type="search"]'));
  });
});
