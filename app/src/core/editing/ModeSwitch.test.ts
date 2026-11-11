// @vitest-environment jsdom
// The buttons plugins put with the mode switch (`editAction`): shown only while the document is edited (Edit |
// Source), each opens its menu below itself.
import { flushSync, mount, unmount, type Component } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import type { MenuEntry } from "../explorer/menu";
import type { MessageKey } from "../i18n";
import { contributions } from "../plugins/instance.svelte";
import ModeSwitch from "./ModeSwitch.svelte";
import { EditingSession } from "./session.svelte";

const Icon: Component<{ size?: number }> = () => ({});

let component: Record<string, unknown> | null = null;
let remove: (() => void) | null = null;
let session: EditingSession;
let target: HTMLElement;
const run = vi.fn();
let offered = true;
let entries: () => MenuEntry[] = () => [{ kind: "item", label: "Fix the selection", run }];

beforeEach(() => {
  session = new EditingSession(createMockApi());
  target = document.createElement("div");
  document.body.append(target);
  offered = true;
  entries = () => [{ kind: "item", label: "Fix the selection", run }];
  remove = contributions.register({
    owner: "core",
    requires: { needs: [], prefers: [] },
    kind: "editAction",
    id: "test.fix",
    label: "app.name",
    icon: Icon,
    when: () => offered,
    entries: () => entries(),
  });
  component = mount(ModeSwitch, {
    target,
    props: { session, readOnly: null, visual: true, choose: () => {} },
  });
  flushSync();
});

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  remove?.();
  remove = null;
  run.mockClear();
  document.body.replaceChildren();
});

const button = (): HTMLButtonElement | null =>
  target.querySelector<HTMLButtonElement>('button[aria-haspopup="menu"]');

describe("edit actions with the mode switch", () => {
  it("are not drawn in View, drawn in Edit and Source", () => {
    expect(button()).toBeNull();
    session.mode = "edit";
    flushSync();
    expect(button()?.getAttribute("aria-label")).toBe("PerMark");
    session.mode = "source";
    flushSync();
    expect(button()).not.toBeNull();
  });

  it("follow their own `when`", () => {
    offered = false;
    session.mode = "edit";
    flushSync();
    expect(button()).toBeNull();
  });

  it("open their menu; an item runs and the menu closes", () => {
    session.mode = "source";
    flushSync();
    button()?.click();
    flushSync();
    expect(button()?.getAttribute("aria-expanded")).toBe("true");
    const item = document.querySelector<HTMLButtonElement>('[role="menuitem"]');
    expect(item?.textContent?.trim()).toBe("Fix the selection");
    item?.click();
    flushSync();
    expect(run).toHaveBeenCalledOnce();
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it("the keyboard starts on the first item that can act now", () => {
    entries = () => [
      { kind: "item", label: "Fix the selection", disabled: true, run },
      { kind: "item", label: "Fix the paragraph", run },
    ];
    session.mode = "source";
    flushSync();
    button()?.click();
    flushSync();
    expect(document.activeElement?.textContent?.trim()).toBe("Fix the paragraph");
  });

  it("stand by their order, lowest first, whichever was added first; those without one first, as added", () => {
    const add = (id: string, label: MessageKey, order?: number): (() => void) =>
      contributions.register({
        owner: "core",
        requires: { needs: [], prefers: [] },
        kind: "editAction",
        id,
        label,
        icon: Icon,
        entries: () => [],
        ...(order === undefined ? {} : { order }),
      });
    const removals = [
      add("test.last", "common.cancel", 2),
      add("test.second", "common.close", 1),
      add("test.plain", "common.details"),
    ];
    session.mode = "edit";
    flushSync();
    const labels = [...target.querySelectorAll('button[aria-haspopup="menu"]')].map((el) =>
      el.getAttribute("aria-label"),
    );
    expect(labels).toEqual(["PerMark", "Details", "Close", "Cancel"]);
    for (const off of removals) off();
  });

  it("a second click on the button closes its menu", () => {
    session.mode = "edit";
    flushSync();
    button()?.click();
    flushSync();
    button()?.click();
    flushSync();
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });
});
