// @vitest-environment jsdom
// The Layouts dialog: what opens it, its rows (Use | Rename | Delete), saving the arrangement shown under a name (a
// kept name: Replace), the reset, what each action says and where the keyboard goes, a write that fails.
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Api, AppSettings } from "../../api";
import type { PluginId } from "../../api/generated/PluginId";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});
// jsdom has no layout: a region's size watcher sees nothing
globalThis.ResizeObserver ??= class {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
};

const { createMockApi } = await import("../../api/mock");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { conditions, neutralContext } = await import("../conditions/context.svelte");
const { ContributionRegistry } = await import("../plugins/registry.svelte");
const { contributions } = await import("../plugins/instance.svelte");
const { registerCoreCommands } = await import("../commands/core");
const { default: LayoutsHost } = await import("../testing/LayoutsHost.svelte");
const { probe } = await import("../testing/ViewProbe.svelte");
const { windowStore } = await import("../window.svelte");
const { layout } = await import("./layout.svelte");
const { arrangement } = await import("./arrangement.svelte");
const { defaultArrangement, FILES, OUTLINE, REGIONS, SEARCH } = await import("./arrangement");
const { ui } = await import("./ui.svelte");
const { viewMenu } = await import("./viewMenu.svelte");

/** A right panel with a header of its own (the probe) and a second one. */
const PROBE = "probe.panel";
const OTHER = "other.panel";

type Groups = readonly (readonly string[])[];
const groups = (list: Groups) =>
  list.map((views) => ({ views: [...views], share: 1 / list.length }));
/** An arrangement with these groups (equal shares) left, right and at the bottom. */
const arranged = (left: Groups, right: Groups, bottom: Groups = []) => ({
  v: 2 as const,
  left: groups(left),
  right: groups(right),
  bottom: groups(bottom),
});
/** As every test starts: the probe over the other panel at the right (the outline, known to every window, is a
 *  tab there: a layout lists every view the window knows). */
const START = arranged([[FILES, SEARCH]], [[PROBE], [OTHER, OUTLINE]]);
/** Apart from it: the probe at the bottom. */
const REVIEW = arranged([[FILES, SEARCH]], [[OTHER]], [[PROBE]]);
/** The two panels as tabs of one group at the right. */
const TABS = arranged([[FILES, SEARCH]], [[OTHER, PROBE]]);

let api: Api;
let component: Record<string, unknown> | null = null;
let stop: () => void = () => {};
const removals: (() => void)[] = [];

function panel(id: string, view: () => Promise<unknown>): void {
  removals.push(
    contributions.register({
      kind: "panel",
      id,
      owner: "test" as PluginId,
      side: "right",
      label: id === PROBE ? "outline.tab" : "live.label",
      requires: NO_REQUIREMENT,
      view: view as never,
    }),
  );
}

/** Where the views of these tests sit now (the outline, known to every window and not present here, left out). */
const placed = (): Record<string, string[][]> =>
  Object.fromEntries(
    REGIONS.map((region) => [
      region,
      arrangement.saved[region]
        .map((g) => g.views.filter((id) => id !== OUTLINE))
        .filter((views) => views.length > 0),
    ]),
  );
const hostOf = (id: string): HTMLElement | null =>
  document.querySelector<HTMLElement>(`[data-view-host="${id}"]`);
const headerButton = (id: string): HTMLElement | null =>
  hostOf(id)?.querySelector<HTMLElement>(`[data-view-menu="${id}"]`) ?? null;

const dialog = (): HTMLElement | null => document.querySelector<HTMLElement>('[role="dialog"]');
const rows = (): HTMLElement[] => [...document.querySelectorAll<HTMLElement>("[data-layout]")];
const rowNames = (): string[] => rows().map((row) => row.getAttribute("data-layout") ?? "");
type Do = "use" | "rename" | "delete";
const rowButton = (at: number, what: Do): HTMLButtonElement => {
  const found = rows()[at]?.querySelector<HTMLButtonElement>(`[data-do="${what}"]`);
  if (!found) throw new Error(`no ${what} button in row ${at}`);
  return found;
};
/** A control of the dialog outside the rows. */
function control<T extends HTMLElement = HTMLButtonElement>(name: string): T {
  const found = dialog()?.querySelector<T>(`[data-do="${name}"]`);
  if (!found) throw new Error(`no ${name}`);
  return found;
}
const nameField = (): HTMLInputElement => control<HTMLInputElement>("name");
const renameField = (): HTMLInputElement => control<HTMLInputElement>("rename-to");
const hint = (): string => dialog()?.querySelector("#layouts-name-hint")?.textContent?.trim() ?? "";
const renameHint = (): string =>
  dialog()?.querySelector("#layouts-rename-hint")?.textContent?.trim() ?? "";
const said = (): string =>
  dialog()?.querySelector("[data-layouts-status]")?.textContent?.trim() ?? "";
const alert = (): string => dialog()?.querySelector('[role="alert"]')?.textContent?.trim() ?? "";
const windowSaid = (): string => document.querySelector("[data-view-status]")?.textContent ?? "";

/** What an action started is over: its write answered, the dialog drawn again, the keyboard placed. */
async function settle(): Promise<void> {
  for (let i = 0; i < 8; i += 1) await tick();
  flushSync();
}

function type(field: HTMLInputElement, text: string): void {
  field.value = text;
  field.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

async function press(el: Element, key: string, init: KeyboardEventInit = {}): Promise<void> {
  el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }));
  await settle();
}

async function click(el: HTMLElement): Promise<void> {
  el.focus();
  el.click();
  await settle();
}

/** The app settings as the backend holds them now. */
async function settings(): Promise<AppSettings> {
  const saved = await api.settingsGet();
  if (!saved.ok) throw new Error("no settings");
  return saved.value;
}

/** The layouts the settings hold: name -> where its views sit. */
async function kept(): Promise<[string, unknown][]> {
  return ((await settings()).layouts?.items ?? []).map((item) => [item.name, item.arrangement]);
}

/** These layouts are in the settings: a name (its arrangement: `REVIEW`), or a name with its arrangement. */
async function keep(...layouts: (string | [string, typeof REVIEW])[]): Promise<void> {
  const items = layouts.map((item) =>
    typeof item === "string"
      ? { name: item, arrangement: REVIEW }
      : { name: item[0], arrangement: item[1] },
  );
  await api.settingsSet({ key: "layouts", value: { v: 1, items } });
  flushSync();
}

/** The window's regions on screen, the probe's view loaded. */
async function show(): Promise<HTMLElement> {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(LayoutsHost, { target });
  flushSync();
  await vi.waitFor(() => expect(hostOf(PROBE)?.querySelector("[data-probe]")).toBeTruthy());
  const host = hostOf(PROBE);
  if (host === null) throw new Error("no probe view");
  return host;
}

/** The dialog, opened from a control that has the keyboard (the probe's options button). */
async function open(): Promise<HTMLElement> {
  headerButton(PROBE)?.focus();
  ui.openLayouts();
  await settle();
  const found = dialog();
  if (found === null) throw new Error("no dialog");
  return found;
}

/** The notice the last change of the arrangement left (with its Undo). */
function notice(): { text: string; undo: () => void } {
  const last = windowStore.notices.at(-1);
  if (last?.kind !== "action" || last.key !== "layout") throw new Error("no layout notice");
  const [action] = last.actions;
  if (action === undefined) throw new Error("no action");
  expect(action.label).toBe("Undo");
  return { text: last.text, undo: () => action.run() };
}

beforeEach(async () => {
  document.body.replaceChildren();
  probe.mounts = 0;
  api = createMockApi();
  panel(PROBE, () => import("../testing/ViewProbe.svelte"));
  panel(OTHER, () => import("../testing/StripFirst.svelte"));
  stop = arrangement.start(api);
  conditions.set({ ...neutralContext(), workspace: "project" });
  await api.projectSessionSet("layout.views", null);
  await layout.load(api, true);
  layout.setOpen(PROBE, true);
  layout.setOpen(OTHER, true);
  arrangement.arrange(START);
  flushSync();
});

afterEach(async () => {
  ui.closeLayouts();
  viewMenu.close();
  windowStore.dismissKey("layout");
  if (component) await unmount(component);
  component = null;
  stop();
  for (const remove of removals.splice(0)) remove();
  conditions.set(neutralContext());
  arrangement.status = "";
  await layout.load(api, false);
  vi.restoreAllMocks();
});

describe("opening the Layouts dialog", () => {
  it("from a view's menu: the entry right before Reset layout; Escape closes it, the keyboard back on the options", async () => {
    await show();
    const button = headerButton(PROBE);
    if (!button) throw new Error("no options button");
    button.focus();
    button.click();
    flushSync();
    const items = [...document.querySelectorAll<HTMLElement>('[role="menu"] [role="menuitem"]')];
    expect(items.map((item) => item.textContent?.trim()).slice(-2)).toEqual([
      "Layouts…",
      "Reset layout",
    ]);
    items.at(-2)?.click();
    await settle();
    expect(document.querySelector('[role="menu"]')).toBeNull();
    const box = dialog();
    expect(box?.getAttribute("aria-modal")).toBe("true");
    expect(document.getElementById(box?.getAttribute("aria-labelledby") ?? "")?.textContent).toBe(
      "Layouts",
    );
    expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(1);
    expect(ui.overlay).toBe("box");
    // no layout yet: one quiet line, the keyboard in the name field
    expect(rows()).toEqual([]);
    expect(box?.querySelector(".empty")?.textContent?.trim()).toBe("No saved layouts yet.");
    expect(document.activeElement).toBe(nameField());

    await press(nameField(), "Escape");
    expect(dialog()).toBeNull();
    expect(ui.overlay).toBe("none");
    expect(document.activeElement).toBe(button);
  });

  it("from the palette: a command of every window that shows a workspace", async () => {
    const registry = new ContributionRegistry();
    const off = registerCoreCommands(registry);
    const command = registry.of("command").find((c) => c.id === "panel.layouts");
    off();
    expect(command?.inPalette).not.toBe(false);
    expect(command?.requires).toEqual({
      needs: [{ need: "workspace", kinds: ["project", "singleFile", "notes"] }],
      prefers: [],
    });
    await show();
    await command?.run();
    await settle();
    expect(dialog()?.querySelector("h2")?.textContent).toBe("Layouts");
  });

  it("the Close button and the backdrop close it; a press inside the dialog does not", async () => {
    await show();
    const box = await open();
    box.click();
    await settle();
    expect(dialog()).not.toBeNull();
    await click(control("close"));
    expect(dialog()).toBeNull();
    expect(document.activeElement).toBe(headerButton(PROBE));
    (await open()).parentElement?.click();
    await settle();
    expect(dialog()).toBeNull();
  });

  it("starts on the first layout's Use; Tab goes through the rows, the name field, Save, Reset to default, Close", async () => {
    await keep("Review", "Writing");
    await show();
    const box = await open();
    expect(document.activeElement).toBe(rowButton(0, "use"));
    type(nameField(), "Other");
    const stops = [...box.querySelectorAll<HTMLElement>("button, input")]
      .filter((el) => !el.matches(":disabled"))
      .map((el) => el.getAttribute("aria-label") ?? el.textContent?.trim() ?? "");
    expect(stops).toEqual([
      "Use layout “Review”",
      "Rename layout “Review”",
      "Delete layout “Review”",
      "Use layout “Writing”",
      "Rename layout “Writing”",
      "Delete layout “Writing”",
      "",
      "Save",
      "Reset to default",
      "Close",
    ]);
    // the field is named by its label
    const label = box.querySelector<HTMLLabelElement>(`label[for="${nameField().id}"]`);
    expect(label?.textContent?.trim()).toBe("Layout name");
    // a row: the name (whole in a tooltip), then what each button says
    const [row] = rows();
    expect(row?.querySelector(".name")?.textContent).toBe("Review");
    expect(row?.querySelector(".name")?.getAttribute("title")).toBe("Review");
    expect(
      (["use", "rename", "delete"] as const).map((b) => rowButton(0, b).textContent?.trim()),
    ).toEqual(["Use", "Rename", "Delete"]);
  });
});

describe("saving the arrangement shown", () => {
  it("under a new name: Enter in the field saves; the row appears, the field is empty again, it is said", async () => {
    await show();
    await open();
    const field = nameField();
    const save = control("save");
    expect(save.disabled).toBe(true);
    expect(save.textContent?.trim()).toBe("Save");
    type(field, "   ");
    expect(save.disabled).toBe(true);
    type(field, "  Review ");
    expect(save.disabled).toBe(false);
    expect(hint()).toBe("");
    // a key of a text being composed is the composition's
    await press(field, "Enter", { isComposing: true });
    expect(rowNames()).toEqual([]);
    await press(field, "Enter");
    expect(rowNames()).toEqual(["Review"]);
    expect(await kept()).toEqual([["Review", START]]);
    expect(field.value).toBe("");
    expect(said()).toBe("Layout “Review” saved");
    expect(dialog()?.querySelector("[data-layouts-status]")?.getAttribute("role")).toBe("status");
    expect(dialog()?.querySelector(".empty")).toBeNull();
    expect(document.activeElement).toBe(field);
  });

  it("with the Save button: the keyboard goes to the field (the button cannot act on an empty name)", async () => {
    await show();
    await open();
    type(nameField(), "Review");
    await click(control("save"));
    expect(rowNames()).toEqual(["Review"]);
    expect(control("save").disabled).toBe(true);
    expect(document.activeElement).toBe(nameField());
  });

  it("under a name a layout has, in any spelling: the button reads Replace, a line says so, one press replaces", async () => {
    await keep("Writing", "Review", "Notes");
    await show();
    await open();
    type(nameField(), " rEVIEW");
    const save = control("save");
    expect(save.textContent?.trim()).toBe("Replace");
    expect(save.disabled).toBe(false);
    expect(hint()).toBe("A layout named “Review” already exists.");
    expect(nameField().getAttribute("aria-describedby")).toBe("layouts-name-hint");
    await click(save);
    // in its place, with the new spelling and the arrangement shown now; no second question
    expect(rowNames()).toEqual(["Writing", "rEVIEW", "Notes"]);
    expect(await kept()).toEqual([
      ["Writing", REVIEW],
      ["rEVIEW", START],
      ["Notes", REVIEW],
    ]);
    expect(said()).toBe("Layout “rEVIEW” saved");
    expect(control("save").textContent?.trim()).toBe("Save");
    expect(hint()).toBe("");
  });

  it("a name that cannot be kept: Save cannot act, a line says what a name is", async () => {
    await show();
    await open();
    type(nameField(), "a".repeat(41));
    expect(control("save").disabled).toBe(true);
    expect(hint()).toBe("Use up to 40 characters, with no tabs or line breaks.");
    await press(nameField(), "Enter");
    expect(rowNames()).toEqual([]);
    type(nameField(), "a".repeat(40));
    expect(control("save").disabled).toBe(false);
    expect(hint()).toBe("");
  });

  it("with 20 layouts kept: a new name cannot be saved and a line says why; a kept one can still be replaced", async () => {
    await keep(...Array.from({ length: 20 }, (_, at) => `L${at + 1}`));
    await show();
    await open();
    expect(rows()).toHaveLength(20);
    expect(hint()).toBe("You can keep up to 20 layouts.");
    type(nameField(), "One more");
    expect(control("save").disabled).toBe(true);
    expect(hint()).toBe("You can keep up to 20 layouts.");
    await press(nameField(), "Enter");
    expect(rows()).toHaveLength(20);
    type(nameField(), "l20");
    expect(control("save").disabled).toBe(false);
    expect(control("save").textContent?.trim()).toBe("Replace");
    expect(hint()).toBe("A layout named “L20” already exists.");
  });
});

describe("renaming a layout", () => {
  it("the row becomes a field with the name selected; Enter keeps the new name, the keyboard back on Rename", async () => {
    await keep("Review", "Writing");
    await show();
    await open();
    await click(rowButton(0, "rename"));
    const field = renameField();
    expect(document.activeElement).toBe(field);
    expect(field.value).toBe("Review");
    expect([field.selectionStart, field.selectionEnd]).toEqual([0, 6]);
    expect(field.getAttribute("aria-label")).toBe("Rename layout “Review”");
    // only that row changed
    expect(rows()[0]?.querySelector('[data-do="use"]')).toBeNull();
    expect(rowButton(1, "use")).toBeTruthy();
    type(field, " Proofing ");
    await press(field, "Enter", { isComposing: true });
    expect(await kept()).toEqual([
      ["Review", REVIEW],
      ["Writing", REVIEW],
    ]);
    await press(field, "Enter");
    expect(rowNames()).toEqual(["Proofing", "Writing"]);
    expect(await kept()).toEqual([
      ["Proofing", REVIEW],
      ["Writing", REVIEW],
    ]);
    expect(dialog()?.querySelector('[data-do="rename-to"]')).toBeNull();
    expect(document.activeElement).toBe(rowButton(0, "rename"));
    expect(said()).toBe("Layout “Proofing” saved");
  });

  it("Escape leaves the rename only: the dialog stays, the name as it was; Cancel does the same", async () => {
    await keep("Review", "Writing");
    await show();
    await open();
    const write = vi.spyOn(api, "settingsSet");
    await click(rowButton(1, "rename"));
    type(renameField(), "Other");
    await press(renameField(), "Escape");
    expect(dialog()).not.toBeNull();
    expect(dialog()?.querySelector('[data-do="rename-to"]')).toBeNull();
    expect(rowNames()).toEqual(["Review", "Writing"]);
    expect(document.activeElement).toBe(rowButton(1, "rename"));
    // Escape on the row's own buttons too
    await click(rowButton(1, "rename"));
    await press(control("rename-cancel"), "Escape");
    expect(dialog()).not.toBeNull();
    expect(dialog()?.querySelector('[data-do="rename-to"]')).toBeNull();
    await click(rowButton(1, "rename"));
    type(renameField(), "Other");
    await click(control("rename-cancel"));
    expect(rowNames()).toEqual(["Review", "Writing"]);
    expect(document.activeElement).toBe(rowButton(1, "rename"));
    expect(write).not.toHaveBeenCalled();
    // then Escape closes the dialog
    await press(rowButton(1, "rename"), "Escape");
    expect(dialog()).toBeNull();
  });

  it("to a name another layout has: the same line, nothing kept; another spelling of its own name is fine", async () => {
    await keep("Review", "Writing");
    await show();
    await open();
    const write = vi.spyOn(api, "settingsSet");
    await click(rowButton(0, "rename"));
    type(renameField(), "WRITING");
    expect(renameHint()).toBe("A layout named “Writing” already exists.");
    expect(control("rename-save").disabled).toBe(true);
    await press(renameField(), "Enter");
    expect(write).not.toHaveBeenCalled();
    expect(renameField().value).toBe("WRITING");
    expect(rowNames()).toEqual(["Review", "Writing"]);
    // not a name: said, nothing kept
    type(renameField(), "a".repeat(41));
    expect(renameHint()).toBe("Use up to 40 characters, with no tabs or line breaks.");
    await press(renameField(), "Enter");
    type(renameField(), "  ");
    expect(renameHint()).toBe("");
    await press(renameField(), "Enter");
    expect(write).not.toHaveBeenCalled();
    // its own name in capitals
    type(renameField(), "REVIEW");
    expect(renameHint()).toBe("");
    await click(control("rename-save"));
    expect(rowNames()).toEqual(["REVIEW", "Writing"]);
    expect(write).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(rowButton(0, "rename"));
  });

  it("to the name it has: nothing is written", async () => {
    await keep("Review");
    await show();
    await open();
    const write = vi.spyOn(api, "settingsSet");
    await click(rowButton(0, "rename"));
    await press(renameField(), "Enter");
    expect(write).not.toHaveBeenCalled();
    expect(dialog()?.querySelector('[data-do="rename-to"]')).toBeNull();
    expect(document.activeElement).toBe(rowButton(0, "rename"));
  });
});

describe("deleting a layout", () => {
  it("at once: the row goes, a line says it with Undo, the keyboard on the next row's Delete (none: the name field)", async () => {
    await keep("A", "B", "C");
    await show();
    await open();
    await click(rowButton(1, "delete"));
    expect(rowNames()).toEqual(["A", "C"]);
    expect((await kept()).map(([name]) => name)).toEqual(["A", "C"]);
    expect(said()).toBe("Layout “B” deleted");
    expect(control("undo").textContent?.trim()).toBe("Undo");
    expect(document.activeElement).toBe(rowButton(1, "delete"));
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Delete layout “C”");
    await click(rowButton(1, "delete"));
    expect(rowNames()).toEqual(["A"]);
    expect(said()).toBe("Layout “C” deleted");
    expect(document.activeElement).toBe(nameField());
  });

  it("Undo puts it back at its place; the line stays until the next action", async () => {
    await keep("A", "B", "C");
    await show();
    await open();
    await click(rowButton(1, "delete"));
    await click(control("undo"));
    expect(rowNames()).toEqual(["A", "B", "C"]);
    expect(await kept()).toEqual([
      ["A", REVIEW],
      ["B", REVIEW],
      ["C", REVIEW],
    ]);
    expect(dialog()?.querySelector('[data-do="undo"]')).toBeNull();
    expect(said()).toBe("Undone.");
    expect(document.activeElement).toBe(rowButton(1, "delete"));

    // the next action takes the line (and its Undo) away
    await click(rowButton(0, "delete"));
    expect(control("undo")).toBeTruthy();
    type(nameField(), "D");
    await press(nameField(), "Enter");
    expect(said()).toBe("Layout “D” saved");
    expect(dialog()?.querySelector('[data-do="undo"]')).toBeNull();
    expect(rowNames()).toEqual(["B", "C", "D"]);
  });
});

describe("using a layout", () => {
  it("arranges the window, closes the dialog, says it; the notice's Undo puts back the arrangement before", async () => {
    await keep("Review");
    const host = await show();
    const typed = host.querySelector<HTMLInputElement>(".probe-field");
    if (!typed) throw new Error("no probe body");
    typed.value = "typed";
    await open();
    await click(rowButton(0, "use"));
    expect(dialog()).toBeNull();
    expect(placed()).toEqual({ left: [[FILES, SEARCH]], right: [[OTHER]], bottom: [[PROBE]] });
    expect(windowSaid()).toBe("Layout “Review” applied");
    // no view was created again
    expect(hostOf(PROBE)).toBe(host);
    expect(host.closest("[data-region]")?.getAttribute("data-region")).toBe("bottom");
    expect(probe.mounts).toBe(1);
    expect(typed.value).toBe("typed");
    // the keyboard is not left on the page body
    expect(document.activeElement).not.toBe(document.body);
    expect(document.activeElement).toBe(headerButton(PROBE));

    const left = notice();
    expect(left.text).toBe("Layout “Review” applied");
    left.undo();
    flushSync();
    expect(placed()).toEqual({ left: [[FILES, SEARCH]], right: [[PROBE], [OTHER]], bottom: [] });
    expect(windowSaid()).toBe("Undone.");
    expect(hostOf(PROBE)).toBe(host);
    expect(probe.mounts).toBe(1);
    expect(typed.value).toBe("typed");
    // the layouts themselves are as they were
    expect(await kept()).toEqual([["Review", REVIEW]]);
  });

  it("opened from a control the new arrangement no longer draws: the keyboard goes to that view, never the body", async () => {
    await keep(["Tabs", TABS]);
    await show();
    // the probe's own header has its options; as a tab of a group the strip has them
    const before = headerButton(PROBE);
    await open();
    await click(rowButton(0, "use"));
    expect(dialog()).toBeNull();
    expect(before?.isConnected).toBe(false);
    expect(document.activeElement).not.toBe(document.body);
    expect(document.activeElement?.closest('[data-region="right"]')).not.toBeNull();
  });

  it("a second layout used: one notice, the newest", async () => {
    await keep("Review", ["Tabs", TABS]);
    await show();
    await open();
    await click(rowButton(0, "use"));
    await open();
    await click(rowButton(1, "use"));
    const mine = windowStore.notices.filter((n) => n.kind === "action" && n.key === "layout");
    expect(mine).toHaveLength(1);
    expect(notice().text).toBe("Layout “Tabs” applied");
    notice().undo();
    expect(placed().bottom).toEqual([[PROBE]]);
  });
});

describe("Reset to default", () => {
  it("cannot act while the arrangement is the default one", async () => {
    arrangement.arrange(defaultArrangement());
    await show();
    await open();
    expect(control("reset").disabled).toBe(true);
    expect(control("reset").textContent?.trim()).toBe("Reset to default");
  });

  it("puts the whole arrangement back, closes the dialog, says it; the notice's Undo puts back the one before", async () => {
    arrangement.arrange(REVIEW);
    await show();
    await open();
    const reset = vi.spyOn(arrangement, "reset");
    expect(control("reset").disabled).toBe(false);
    await click(control("reset"));
    expect(reset).toHaveBeenCalledOnce();
    expect(dialog()).toBeNull();
    expect(arrangement.atDefault).toBe(true);
    expect(windowSaid()).toBe("Layout of the panels reset");
    expect(notice().text).toBe("Layout of the panels reset");
    notice().undo();
    expect(placed()).toEqual({ left: [[FILES, SEARCH]], right: [[OTHER]], bottom: [[PROBE]] });
    expect(arrangement.status).toBe("Undone.");
  });
});

describe("a write that fails", () => {
  const error = {
    kind: "backend" as const,
    command: "settings_set" as const,
    error: { kind: "invalidData" as const, subject: "layouts", detail: null },
  };

  it("on a save: the dialog stays, the error is said in words, the name stays in the field, no row", async () => {
    await show();
    await open();
    vi.spyOn(api, "settingsSet").mockResolvedValue({ ok: false, error });
    type(nameField(), "Review");
    await press(nameField(), "Enter");
    expect(dialog()).not.toBeNull();
    expect(alert()).toBe("The data isn't valid: layouts");
    expect(rowNames()).toEqual([]);
    expect(nameField().value).toBe("Review");
    expect(said()).toBe("");
    // the next action that works takes the error away
    vi.restoreAllMocks();
    await press(nameField(), "Enter");
    expect(alert()).toBe("");
    expect(rowNames()).toEqual(["Review"]);
  });

  it("on a delete or a rename: the row is as it was, the keyboard on the button pressed", async () => {
    await keep("A", "B");
    await show();
    await open();
    vi.spyOn(api, "settingsSet").mockResolvedValue({ ok: false, error });
    await click(rowButton(0, "delete"));
    expect(rowNames()).toEqual(["A", "B"]);
    expect(alert()).toBe("The data isn't valid: layouts");
    expect(dialog()?.querySelector('[data-do="undo"]')).toBeNull();
    expect(document.activeElement).toBe(rowButton(0, "delete"));
    await click(rowButton(1, "rename"));
    expect(alert()).toBe("");
    type(renameField(), "C");
    await press(renameField(), "Enter");
    expect(rowNames()).toEqual(["A", "B"]);
    expect(alert()).toBe("The data isn't valid: layouts");
    expect(document.activeElement).toBe(rowButton(1, "rename"));
  });

  it("on the Undo of a delete: the Undo stays to try again", async () => {
    await keep("A", "B");
    await show();
    await open();
    await click(rowButton(0, "delete"));
    vi.spyOn(api, "settingsSet").mockResolvedValue({ ok: false, error });
    await click(control("undo"));
    expect(rowNames()).toEqual(["B"]);
    expect(alert()).toBe("The data isn't valid: layouts");
    vi.restoreAllMocks();
    await click(control("undo"));
    expect(rowNames()).toEqual(["A", "B"]);
    expect(alert()).toBe("");
  });
});
