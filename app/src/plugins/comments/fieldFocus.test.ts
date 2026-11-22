// @vitest-environment jsdom
// The comment field gives the keyboard back to where the person selected the passage once it closes: the page (no
// focus ring flashing around it: the person never left it) or the editor.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { comments } = await import("./store.svelte");
const { SENT } = await import("../../core/doc/paneFocus");
const { default: CommentHost } = await import("./CommentHost.svelte");

let scroller: HTMLElement;
let component: Record<string, unknown> | null = null;

beforeEach(() => {
  scroller = document.createElement("div");
  scroller.dataset["scroller"] = "";
  scroller.tabIndex = -1;
  const target = document.createElement("div");
  document.body.append(scroller, target);
  vi.spyOn(comments, "save").mockImplementation(() => {
    comments.draft = null;
    return Promise.resolve(true);
  });
  component = mount(CommentHost, { target });
  flushSync();
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  comments.draft = null;
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

/** Writes a comment on a passage and saves it with Enter, the keyboard on `from` before. */
async function writeFrom(from: HTMLElement): Promise<void> {
  from.focus();
  comments.draft = {
    kind: "new",
    path: "docs/guide.md",
    quote: "Back to the readme",
    headingPath: ["Guide"],
    line: 3,
    endLine: 3,
    rect: new DOMRect(40, 120, 200, 18),
  };
  flushSync();
  const box = document.querySelector<HTMLTextAreaElement>('[role="dialog"] textarea');
  if (!box) throw new Error("no field");
  expect(document.activeElement).toBe(box);
  box.value = "Say it better";
  box.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  box.dispatchEvent(
    new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
  );
  await vi.waitFor(() => expect(comments.draft).toBeNull());
  flushSync();
}

it("saved with Enter from a passage of the page: the page's pane holds the keyboard again, not marked to draw a ring", async () => {
  await writeFrom(scroller);
  expect(document.activeElement).toBe(scroller);
  expect(scroller.hasAttribute(SENT)).toBe(false);
});

it("saved with Enter from the editor: the editor has the keyboard again", async () => {
  const editor = document.createElement("div");
  editor.setAttribute("contenteditable", "true");
  editor.tabIndex = 0;
  scroller.append(editor);
  await writeFrom(editor);
  expect(document.activeElement).toBe(editor);
});
