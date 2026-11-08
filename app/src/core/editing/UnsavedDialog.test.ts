// @vitest-environment jsdom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import UnsavedDialog from "./UnsavedDialog.svelte";

type Choice = "save" | "discard" | "keepEditing";

let target: HTMLElement;
let component: Record<string, unknown> | null = null;
let answers: Choice[] = [];

beforeEach(() => {
  document.body.replaceChildren();
  // what had the keyboard before: the editor behind
  const editor = document.createElement("div");
  editor.contentEditable = "true";
  editor.tabIndex = 0;
  document.body.append(editor);
  editor.focus();
  target = document.createElement("div");
  document.body.append(target);
  answers = [];
});

afterEach(() => {
  if (component) void unmount(component);
  component = null;
});

function open(canSave = true): HTMLElement {
  component = mount(UnsavedDialog, {
    target,
    props: { name: "guide.md", canSave, answer: (choice: Choice) => void answers.push(choice) },
  });
  flushSync();
  const dialog = target.querySelector<HTMLElement>("[role=dialog]");
  if (!dialog) throw new Error("no dialog");
  return dialog;
}

const button = (label: string): HTMLButtonElement | undefined =>
  [...target.querySelectorAll("button")].find((b) => b.textContent?.trim() === label);

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

describe("the unsaved changes question", () => {
  it("is a named modal dialog that takes the keyboard on Keep editing", () => {
    const dialog = open();
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(document.getElementById(dialog.getAttribute("aria-labelledby") ?? "")?.textContent).toBe(
      "Unsaved changes",
    );
    expect(dialog.textContent).toContain("guide.md");
    expect(document.activeElement).toBe(button("Keep editing"));
  });

  it("Tab | Shift+Tab stay inside it", () => {
    open();
    const first = button("Save");
    const last = button("Keep editing");
    if (!first || !last) throw new Error("no buttons");
    expect(key(last, "Tab").defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(first);
    expect(key(first, "Tab", { shiftKey: true }).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(last);
  });

  it("Escape keeps editing; the buttons give their answer", () => {
    const dialog = open();
    const escape = key(dialog, "Escape");
    expect(escape.defaultPrevented).toBe(true);
    button("Save")?.click();
    button("Discard")?.click();
    button("Keep editing")?.click();
    expect(answers).toEqual(["keepEditing", "save", "discard", "keepEditing"]);
  });

  it("an Escape that ends a composition is not an answer", () => {
    const dialog = open();
    key(dialog, "Escape", { isComposing: true });
    expect(answers).toEqual([]);
  });

  it("a press on the backdrop keeps the focus in the dialog", () => {
    open();
    const backdrop = target.querySelector(".backdrop");
    const down = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    backdrop?.dispatchEvent(down);
    expect(down.defaultPrevented).toBe(true);
  });

  it("a document in conflict offers no Save", () => {
    open(false);
    expect(button("Save")).toBeUndefined();
    expect(document.activeElement).toBe(button("Keep editing"));
  });
});
