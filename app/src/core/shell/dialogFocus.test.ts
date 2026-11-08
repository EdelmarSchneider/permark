// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { dialogFocus, type DialogFocusOptions } from "./dialogFocus";

let opener: HTMLButtonElement;
let handles: { destroy?: () => void }[] = [];

beforeEach(() => {
  document.body.replaceChildren();
  opener = document.createElement("button");
  opener.textContent = "Open";
  document.body.append(opener);
  opener.focus();
});

afterEach(() => {
  for (const h of handles) h.destroy?.();
  handles = [];
});

/** A dialog with `html` inside, put on the page and given the action. */
function open(html: string, options: DialogFocusOptions = {}) {
  const dialog = document.createElement("div");
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("aria-modal", "true");
  dialog.tabIndex = -1;
  dialog.innerHTML = html;
  document.body.append(dialog);
  const handle = dialogFocus(dialog, options);
  handles.push(handle);
  const close = (): void => {
    handle.destroy?.();
    handles = handles.filter((h) => h !== handle);
    dialog.remove();
  };
  return { dialog, handle, close };
}

const byId = (id: string): HTMLElement => {
  const el = document.getElementById(id);
  if (!el) throw new Error(`no #${id}`);
  return el;
};

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

const THREE = `<button id="a">A</button><input id="b" /><button id="c">C</button>`;

describe("focus on open", () => {
  it("goes to the first control by default", () => {
    open(THREE);
    expect(document.activeElement).toBe(byId("a"));
  });

  it("goes to the control the selector names", () => {
    open(`<button id="a">A</button><button id="c" data-initial>C</button>`, {
      initial: "[data-initial]",
    });
    expect(document.activeElement).toBe(byId("c"));
  });

  it("goes to the dialog itself when asked (a reading surface)", () => {
    const { dialog } = open(THREE, { initial: (d) => d });
    expect(document.activeElement).toBe(dialog);
  });

  it("a disabled default answer passes it to the first control", () => {
    open(`<button id="a">A</button><button id="c" disabled data-initial>C</button>`, {
      initial: "[data-initial]",
    });
    expect(document.activeElement).toBe(byId("a"));
  });

  it("skips what Tab never stops at; nothing to stop at = the dialog", () => {
    open(
      `<button id="d" disabled>D</button><button id="h" hidden>H</button>` +
        `<div inert><button id="i">I</button></div><span id="r" tabindex="-1">R</span>` +
        `<input type="hidden" /><a id="l" href="#x">L</a>`,
    );
    expect(document.activeElement).toBe(byId("l"));
    document.body.replaceChildren(opener);
    const { dialog } = open(`<p>Only words</p><button disabled>D</button>`);
    expect(document.activeElement).toBe(dialog);
  });
});

describe("Tab stays inside", () => {
  it("wraps at both ends and leaves the middle to the browser", () => {
    open(THREE);
    const [a, b, c] = [byId("a"), byId("b"), byId("c")];
    c.focus();
    expect(key(c, "Tab").defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(a);
    expect(key(a, "Tab", { shiftKey: true }).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(c);
    b.focus();
    expect(key(b, "Tab").defaultPrevented).toBe(false);
    expect(key(b, "Tab", { shiftKey: true }).defaultPrevented).toBe(false);
  });

  it("Shift+Tab on the dialog itself goes to the last control, not the page behind", () => {
    const { dialog } = open(THREE, { initial: (d) => d });
    expect(key(dialog, "Tab", { shiftKey: true }).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(byId("c"));
  });

  it("with no control, Tab keeps the focus on the dialog", () => {
    const { dialog } = open(`<img alt="" />`);
    expect(key(dialog, "Tab").defaultPrevented).toBe(true);
    expect(key(dialog, "Tab", { shiftKey: true }).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(dialog);
  });

  it("a dialog that gives Tab its own meaning keeps it", () => {
    open(THREE, { trapTab: false });
    const c = byId("c");
    c.focus();
    expect(key(c, "Tab").defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(c);
  });

  it("a control that already used Tab keeps it", () => {
    open(THREE);
    const c = byId("c");
    c.addEventListener("keydown", (e) => e.preventDefault());
    c.focus();
    key(c, "Tab");
    expect(document.activeElement).toBe(c);
  });
});

describe("Escape", () => {
  it("is the dialog's cancel and goes no further", () => {
    let cancelled = 0;
    let outside = 0;
    document.body.addEventListener("keydown", () => void (outside += 1));
    const { dialog } = open(THREE, { onEscape: () => void (cancelled += 1) });
    expect(key(dialog, "Escape").defaultPrevented).toBe(true);
    expect(cancelled).toBe(1);
    expect(outside).toBe(0);
  });

  it("is not an answer while a composition ends, nor when a control inside used it", () => {
    let cancelled = 0;
    open(THREE, { onEscape: () => void (cancelled += 1) });
    const b = byId("b");
    key(b, "Escape", { isComposing: true });
    b.addEventListener("keydown", (e) => e.preventDefault(), { once: true });
    key(b, "Escape");
    expect(cancelled).toBe(0);
  });

  it("passes through without a cancel; a changed cancel is the one called", () => {
    const { dialog, handle } = open(THREE);
    expect(key(dialog, "Escape").defaultPrevented).toBe(false);
    const said: string[] = [];
    handle.update?.({ onEscape: () => void said.push("new") });
    key(dialog, "Escape");
    expect(said).toEqual(["new"]);
  });
});

describe("focus on close", () => {
  it("goes back to what had it before", () => {
    const { close } = open(THREE);
    expect(document.activeElement).toBe(byId("a"));
    close();
    expect(document.activeElement).toBe(opener);
  });

  it("stays where the host put it when asked, and never goes to an opener that is gone", () => {
    const { close } = open(THREE, { restoreFocus: false });
    close();
    expect(document.activeElement).not.toBe(opener);
    opener.focus();
    const second = open(THREE);
    opener.remove();
    expect(() => second.close()).not.toThrow();
    expect(document.activeElement).not.toBe(opener);
  });
});
