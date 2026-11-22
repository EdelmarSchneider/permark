// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import { t } from "../i18n";
import { WindowStore } from "../window.svelte";
import Notices from "./Notices.svelte";

let component: Record<string, unknown> | null = null;

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  document.body.replaceChildren();
});

function shown(rightInset: number): { store: WindowStore; target: HTMLElement } {
  const target = document.createElement("div");
  document.body.append(target);
  const store = new WindowStore(createMockApi());
  component = mount(Notices, { target, props: { store, rightInset } });
  store.notifyNote("Saved, 1 change.");
  flushSync();
  return { store, target };
}

describe("the notices", () => {
  it("stand clear of the right panel (Live changes): they keep to the left of its width", () => {
    const { target } = shown(300);
    const region = target.querySelector<HTMLElement>(".notices");
    expect(region?.style.getPropertyValue("--notices-right")).toBe("300px");
  });

  it("keep their live region on the page before the first notice, so screen readers hear that one too", () => {
    const target = document.createElement("div");
    document.body.append(target);
    const store = new WindowStore(createMockApi());
    component = mount(Notices, { target, props: { store } });
    flushSync();
    const region = target.querySelector<HTMLElement>('[role="status"][aria-live="polite"]');
    if (!region) throw new Error("no live region before the first notice");
    expect(region.textContent?.trim()).toBe("");
    // no notice takes no room at the bottom of the pages
    expect(document.documentElement.style.getPropertyValue("--notices-h")).toBe("");
    store.notifyNote("Saved, 1 change.");
    flushSync();
    // the same element: the words land inside the region the screen reader already watches
    expect(target.querySelector('[role="status"]')).toBe(region);
    expect(region.textContent).toContain("Saved, 1 change.");
    expect(document.documentElement.style.getPropertyValue("--notices-h")).not.toBe("");
    for (const notice of store.notices) store.dismiss(notice.id, "dismissed");
    flushSync();
    expect(target.querySelector('[role="status"]')).toBe(region);
    expect(document.documentElement.style.getPropertyValue("--notices-h")).toBe("");
  });

  it("are told politely to screen readers, and their buttons take the keyboard", () => {
    const { store, target } = shown(0);
    store.notifyAction("Deleted a.md", { label: "Undo", run: () => {} });
    flushSync();
    const region = target.querySelector(".notices");
    expect(region?.getAttribute("aria-live")).toBe("polite");
    const undo = Array.from(target.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Undo",
    );
    expect(undo?.tabIndex).toBe(0);
  });

  it("tell screen readers the shortcut that does what a button does, on that button only", () => {
    const { store, target } = shown(0);
    store.notifyAction(
      "Deleted a.md",
      { label: "Undo", run: () => {}, keys: "Control+Z" },
      { label: "Open", run: () => {} },
    );
    flushSync();
    const button = (label: string): HTMLButtonElement | undefined =>
      Array.from(target.querySelectorAll("button")).find((b) => b.textContent?.trim() === label);
    expect(button("Undo")?.getAttribute("aria-keyshortcuts")).toBe("Control+Z");
    expect(button("Open")?.hasAttribute("aria-keyshortcuts")).toBe(false);
  });

  it("say which folder a refused Initialize left as it was", async () => {
    const target = document.createElement("div");
    document.body.append(target);
    const store = new WindowStore({
      ...createMockApi(),
      projectInit: () =>
        Promise.resolve({
          ok: false,
          error: {
            kind: "backend",
            command: "project_init",
            error: { kind: "conflict", subject: "C:/a/b/.permark/local", detail: "bothHoldData" },
          },
        }),
    });
    component = mount(Notices, { target, props: { store } });
    expect(await store.initProject("C:/a/b")).toBe(false);
    flushSync();
    expect(target.querySelector(".notice .text")?.textContent).toBe(
      t("project.initHeld", { path: "C:/a/b" }),
    );
  });
});

describe("after a notice's button ran", () => {
  /** A control of the window, outside the notices. */
  function control(name: string): HTMLButtonElement {
    const button = document.createElement("button");
    button.textContent = name;
    document.body.append(button);
    return button;
  }
  /** The page region, as the frame draws it. */
  function page(): HTMLElement {
    const main = document.createElement("main");
    main.setAttribute("data-region", "page");
    main.tabIndex = -1;
    document.body.append(main);
    return main;
  }
  function buttonOf(target: HTMLElement, label: string): HTMLButtonElement {
    const found = Array.from(target.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === label,
    );
    if (!found) throw new Error(`no button ${label}`);
    return found;
  }
  /** Every pending step is done (the last notice going takes the whole region with it: more than two ticks). */
  const drawn = (): Promise<void> => new Promise((done) => setTimeout(done, 0));
  /** The keyboard goes to the button, the button is pressed, what that changed is drawn. */
  async function pressed(button: HTMLButtonElement): Promise<void> {
    button.focus();
    button.click();
    flushSync();
    await tick();
    await tick();
  }

  it("the keyboard goes back to what had it before it came into the notice, never the page body", async () => {
    const outside = control("Options");
    page();
    const { store, target } = shown(0);
    const ran = vi.fn();
    store.notifyAction("Deleted a.md", { label: "Undo", run: ran });
    flushSync();
    outside.focus();
    const undo = buttonOf(target, "Undo");
    await pressed(undo);
    expect(ran).toHaveBeenCalledOnce();
    expect(undo.isConnected).toBe(false);
    expect(document.activeElement).toBe(outside);
  });

  it("through another notice first: still what had it before the notices", async () => {
    const outside = control("Options");
    const { store, target } = shown(0);
    store.notifyAction("Deleted a.md", { label: "Undo", run: () => {} });
    flushSync();
    outside.focus();
    // the first notice's Dismiss, then on to the second notice's Undo
    buttonOf(target, "Dismiss").focus();
    await pressed(buttonOf(target, "Undo"));
    expect(document.activeElement).toBe(outside);
  });

  it("what had it is gone with what the action changed: the page takes it", async () => {
    const outside = control("Options");
    const main = page();
    const { store, target } = shown(0);
    store.notifyAction("Layout of the panels reset", {
      label: "Undo",
      run: () => outside.remove(),
    });
    flushSync();
    outside.focus();
    await pressed(buttonOf(target, "Undo"));
    expect(document.activeElement).toBe(main);
  });

  it("nothing had it (a press with the pointer, the keyboard on the page body): the page takes it", async () => {
    const main = page();
    const { store, target } = shown(0);
    store.notifyAction("Deleted a.md", { label: "Undo", run: () => {} });
    flushSync();
    expect(document.activeElement).toBe(document.body);
    await pressed(buttonOf(target, "Undo"));
    expect(document.activeElement).toBe(main);
  });

  it("an action whose work goes on is waited for: the place it gives the keyboard at its end is kept", async () => {
    const outside = control("Options");
    const asked = control("Include all");
    page();
    const { store, target } = shown(0);
    let finish: () => void = () => {};
    const work = new Promise<void>((done) => (finish = done));
    store.notifyAction("Included 4 projects", {
      label: "Undo",
      // as a feature does after its backend answered: the keyboard to its own control, when nothing holds it
      run: () =>
        work.then(() => {
          if (document.activeElement === document.body) asked.focus();
        }),
    });
    flushSync();
    outside.focus();
    await pressed(buttonOf(target, "Undo"));
    // not yet done: nothing takes the keyboard meanwhile
    expect(document.activeElement).toBe(document.body);
    finish();
    await tick();
    await tick();
    expect(document.activeElement).toBe(asked);
  });

  it("Dismiss (Enter | a click) and Esc take the notice away the same way: the keyboard goes back", async () => {
    const outside = control("Options");
    page();
    const { store, target } = shown(0);
    store.notifyAction("Deleted a.md", { label: "Undo", run: () => {} });
    flushSync();
    outside.focus();
    const dismiss = buttonOf(target, "Dismiss");
    await pressed(dismiss);
    expect(dismiss.isConnected).toBe(false);
    expect(document.activeElement).toBe(outside);

    outside.focus();
    const undo = buttonOf(target, "Undo");
    undo.focus();
    const esc = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    undo.dispatchEvent(esc);
    flushSync();
    await drawn();
    expect(esc.defaultPrevented).toBe(true);
    expect(undo.isConnected).toBe(false);
    expect(document.activeElement).toBe(outside);
  });

  it("Esc on a notice when what had the keyboard is gone: the page takes it", async () => {
    const outside = control("Options");
    const main = page();
    const { target } = shown(0);
    outside.focus();
    const dismiss = buttonOf(target, "Dismiss");
    dismiss.focus();
    outside.remove();
    dismiss.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    flushSync();
    await drawn();
    expect(document.activeElement).toBe(main);
  });

  it("an action that puts the keyboard somewhere keeps it there", async () => {
    const outside = control("Options");
    const other = control("Opened");
    page();
    const { store, target } = shown(0);
    store.notifyAction("Moved a.md", { label: "Open", run: () => other.focus() });
    flushSync();
    outside.focus();
    await pressed(buttonOf(target, "Open"));
    expect(document.activeElement).toBe(other);
  });
});
