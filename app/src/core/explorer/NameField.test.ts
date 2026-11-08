// @vitest-environment jsdom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import NameField from "./NameField.svelte";

const NOTE = ".md is added. End the name with .txt for a plain text file.";
let component: Record<string, unknown> | null = null;

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  document.body.replaceChildren();
});

function field(value: string, problem: string | null = null): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(NameField, {
    target,
    props: {
      value,
      label: "Name of the new file",
      hint: "Enter creates the file, Esc cancels",
      problem,
      note: NOTE,
      suffix: ".md",
      oninput: () => {},
      commit: () => {},
      cancel: () => {},
    },
  });
  flushSync();
  return target;
}

describe("the name field of a new file", () => {
  it("says how the file is named within its own row: nothing covers the rows below", () => {
    const target = field("");
    const input = target.querySelector("input")!;
    const note = document.getElementById(input.getAttribute("aria-describedby") ?? "");
    expect(note?.textContent).toBe(NOTE);
    expect(note?.classList.contains("sr-only")).toBe(true);
    expect(input.title).toContain(NOTE);
    expect(target.querySelector(".suffix")?.textContent).toBe(".md");
  });

  it("a name with its own extension shows no .md after it", () => {
    const target = field("plain.txt");
    expect(target.querySelector(".suffix")).toBeNull();
  });

  it("a problem with the name still shows under it", () => {
    const target = field("a:b", "These characters cannot be used.");
    expect(target.querySelector("[role=alert]")?.textContent).toBe(
      "These characters cannot be used.",
    );
  });
});
