// @vitest-environment jsdom
// The Reading settings hold app-wide choices only: reading width and zoom are per document view and live in the
// document header's reading menu, so this page has no row for them.
import "../../testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { windowStore } from "../../window.svelte";
import ReadingSection from "./ReadingSection.svelte";

let component: Record<string, unknown> | null = null;
let root: HTMLElement;

beforeEach(() => {
  root = document.body.appendChild(document.createElement("div"));
  component = mount(ReadingSection, { target: root, props: { store: windowStore } });
  flushSync();
});

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
});

describe("the Reading settings", () => {
  it("keeps language, theme, light tone and wrap", () => {
    expect(root.querySelector("#set-language")).not.toBeNull();
    expect(root.querySelector("#set-theme")).not.toBeNull();
    expect(root.querySelector("#set-tone")).not.toBeNull();
    expect(root.querySelector("#set-wrap")).not.toBeNull();
  });

  it("has no reading width or zoom row", () => {
    expect(root.querySelector("#set-width")).toBeNull();
    expect(root.querySelector("#set-zoom")).toBeNull();
    expect(root.textContent).not.toContain("Kept for each project");
    expect(root.textContent).not.toContain("Open a document to change the width");
  });
});
