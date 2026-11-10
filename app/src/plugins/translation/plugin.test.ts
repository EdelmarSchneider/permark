// @vitest-environment jsdom
// Plugin `translation` switched on | off: the review follows the model of job "translation" and the remembered
// languages, and nothing is left after; dormant while no model is assigned to the job. Its ways in while a document
// is edited: the button with the mode switch and its menu, the commands and the key; a first run asks for the
// language; none of them runs while another plugin's review is open.
import "../../core/testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MenuEntry } from "../../core/explorer/menu";
import type { Found, ReviewState } from "../local-ai/pieces/review.svelte";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { ContributionRegistry, PluginHost, pluginLoaders } =
  await import("../../core/plugins/registry.svelte");
const { editing } = await import("../../core/editing/session.svelte");
const { translation } = await import("./store.svelte");
const { languageDialog } = await import("./dialog.svelte");
const { correction } = await import("../correction/store.svelte");
const { t } = await import("../../core/i18n");
const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;

type Registry = InstanceType<typeof ContributionRegistry>;
type Item = Extract<MenuEntry, { kind: "item" }>;

const S1: Found = {
  key: 1,
  from: 0,
  to: 5,
  original: "Hello",
  insert: "Hallo",
  before: "",
  after: "",
};
const S2: Found = { ...S1, key: 2, from: 10, to: 15 };

/** A review with `suggestions` to go through, its run ended. */
function reviewOf(...suggestions: Found[]): Extract<ReviewState, { kind: "review" }> {
  return {
    kind: "review",
    status: { kind: "finished" },
    suggestions,
    at: 0,
    discarded: 0,
    same: 0,
    outdated: 0,
  };
}

let host: InstanceType<typeof PluginHost> | null = null;

afterEach(() => {
  host?.disable("translation");
  host?.disable("correction");
  host = null;
  mock.probe.translation.available = null;
  mock.probe.translation.prefs = { target: null, recent: [] };
  editing.mode = "view";
  correction.state = { kind: "closed" };
  translation.state = { kind: "closed" };
  languageDialog.close();
  vi.restoreAllMocks();
});

/** The plugin on (after `first`, when given), in a window whose model for the job is there. */
async function on(...first: "correction"[]) {
  const registry = new ContributionRegistry();
  host = new PluginHost(registry, pluginLoaders());
  for (const id of first) await host.enable(id);
  await host.enable("translation");
  return { registry, host };
}

/** The plugin on, a model on its job, the open document edited, `target` the language chosen last. */
async function editingWith(target: string | null, ...first: "correction"[]) {
  mock.probe.translation.prefs = { target, recent: target === null ? [] : [target] };
  const { registry } = await on(...first);
  await vi.waitFor(() => expect(translation.available).toBe(true));
  await vi.waitFor(() => expect(translation.languages.target).toBe(target));
  editing.mode = "source";
  return registry;
}

function command(registry: Registry, id: string) {
  const found = registry.of("command").find((c) => c.id === id);
  if (found === undefined) throw new Error(`no command ${id}`);
  return found;
}

/** The entries of a plugin's button with the mode switch. */
function menuOf(registry: Registry, id = "translation.menu"): MenuEntry[] {
  const action = registry.of("editAction").find((a) => a.id === id);
  if (action === undefined) throw new Error(`no button ${id}`);
  return action.entries();
}

const itemsOf = (entries: MenuEntry[]): Item[] =>
  entries.filter((entry): entry is Item => entry.kind === "item");

function item(registry: Registry, label: string): Item {
  const found = itemsOf(menuOf(registry)).find((entry) => entry.label === label);
  if (found === undefined) throw new Error(`no menu item ${label}`);
  return found;
}

describe("plugin translation", () => {
  it("switched on it follows the job's model and the remembered languages; switched off nothing is left", async () => {
    mock.probe.translation.prefs = { target: "pt-BR", recent: ["pt-BR", "de"] };
    const { registry, host } = await on();
    expect(host.state("translation")).toBe("on");
    await vi.waitFor(() => expect(translation.available).toBe(true));
    await vi.waitFor(() => expect(translation.languages.target).toBe("pt-BR"));
    expect(translation.languages.recent).toEqual(["pt-BR", "de"]);
    expect(host.disable("translation"), "contributions left after deactivate").toBe(0);
    expect(registry.items).toHaveLength(0);
    expect(translation.available).toBe(false);
    expect(translation.languages).toMatchObject({ target: null, source: null, recent: [] });
  });

  it("is dormant until a model is assigned to the job", async () => {
    mock.probe.translation.available = false;
    const { host } = await on();
    await vi.waitFor(() => expect(mock.probe.translation.calls).toContain("status"));
    expect(translation.available).toBe(false);
    host.disable("translation");
  });

  it("adds the command that opens the language dialog and the dialog's place; off, both go and the dialog closes", async () => {
    const { registry, host } = await on();
    await vi.waitFor(() => expect(translation.available).toBe(true));
    expect(registry.ownedBy("translation").length).toBeGreaterThan(0);
    expect(registry.of("statusItem").map((c) => c.id)).toContain("translation.host");
    const language = command(registry, "translation.language");
    expect(language.when?.()).toBe(true);
    expect(t(language.label)).toBe("Translate to…");
    expect(languageDialog.state.status).toBe("closed");
    void language.run();
    // the default: the choice is saved, nothing else follows
    expect(languageDialog.state).toMatchObject({
      status: "open",
      confirm: "translation.language.save",
    });
    expect(host.disable("translation"), "contributions left after deactivate").toBe(0);
    expect(registry.items).toHaveLength(0);
    expect(languageDialog.state.status).toBe("closed");
  });

  it("nothing is offered until a model is assigned to the job: no button, no command", async () => {
    mock.probe.translation.available = false;
    const { registry, host } = await on();
    await vi.waitFor(() => expect(mock.probe.translation.calls).toContain("status"));
    editing.mode = "source";
    const offered = registry
      .ownedBy("translation")
      .filter((c) => c.kind === "command" || c.kind === "editAction");
    expect(offered.map((c) => c.id)).toEqual(
      expect.arrayContaining(["translation.language", "translation.menu", "translation.here"]),
    );
    for (const entry of offered) expect(entry.when?.(), entry.id).toBe(false);
    host.disable("translation");
  });

  it("the dialog's place draws the dialog over the window while something asks for it", async () => {
    const { registry, host } = await on();
    const place = registry.of("statusItem").find((c) => c.id === "translation.host");
    const { default: Place } = (await place?.view()) ?? { default: null };
    if (Place === null) throw new Error("no place");
    const target = document.createElement("div");
    document.body.append(target);
    const app = mount(Place, { target });
    flushSync();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    languageDialog.ask();
    flushSync();
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    // the plugin goes: so does its dialog
    host.disable("translation");
    flushSync();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    await unmount(app);
    target.remove();
  });

  it("asks again when the Local AI assignments change", async () => {
    mock.probe.translation.available = false;
    const { host } = await on();
    await vi.waitFor(() => expect(mock.probe.translation.calls).toContain("status"));
    mock.probe.translation.available = null;
    await mock.pluginCall("local-ai.assign", { task: "translation", model: "gemma-4-e2b-it-q4_0" });
    await vi.waitFor(() => expect(translation.available).toBe(true));
    host.disable("translation");
  });
});

describe("the ways into a translation", () => {
  it("a button with the mode switch, the review bar, the commands and the key; off, nothing is left", async () => {
    const registry = await editingWith("de", "correction");
    expect([...new Set(registry.ownedBy("translation").map((c) => c.kind))].sort()).toEqual([
      "command",
      "docBar",
      "editAction",
      "shortcut",
      "statusItem",
    ]);
    const own = registry.ownedBy("translation");
    expect(own.flatMap((c) => (c.kind === "shortcut" ? [`${c.command} ${c.keys}`] : []))).toEqual([
      "translation.here Mod+Alt+t",
    ]);
    expect(own.flatMap((c) => (c.kind === "command" ? [c.id] : [])).sort()).toEqual([
      "translation.document",
      "translation.here",
      "translation.language",
      "translation.next",
      "translation.prev",
    ]);
    // the button after correction's, the bar right under correction's
    const [button] = registry.of("editAction").filter((a) => a.owner === "translation");
    const [corrects] = registry.of("editAction").filter((a) => a.owner === "correction");
    expect(t(button?.label ?? "app.name")).toBe("Translate");
    expect(button?.command).toBe("translation.here");
    expect(button?.order ?? 0).toBeGreaterThan(corrects?.order ?? 0);
    const bars = [...registry.of("docBar")].sort((a, b) => a.order - b.order);
    expect(bars.map((bar) => bar.id)).toEqual(["correction.bar", "translation.bar"]);
    expect(host?.disable("translation"), "contributions left after deactivate").toBe(0);
    expect(registry.ownedBy("translation")).toHaveLength(0);
  });

  it("are offered only while the open document is edited", async () => {
    const registry = await editingWith("de");
    const [button] = registry.of("editAction");
    const run = ["translation.here", "translation.document"].map((id) => command(registry, id));
    for (const mode of ["edit", "source"] as const) {
      editing.mode = mode;
      expect(button?.when?.(), mode).toBe(true);
      for (const entry of run) expect(entry.when?.(), `${entry.id} ${mode}`).toBe(true);
    }
    editing.mode = "view";
    expect(button?.when?.()).toBe(false);
    for (const entry of run) expect(entry.when?.(), entry.id).toBe(false);
    expect(t(command(registry, "translation.here").label)).toBe(
      "Translate the selection or this paragraph",
    );
    expect(t(command(registry, "translation.document").label)).toBe("Translate the whole document");
  });

  it("the menu: selection (only with one), this paragraph, whole document, then the language translated into", async () => {
    const registry = await editingWith("de");
    const entries = menuOf(registry);
    expect(entries.map((entry) => (entry.kind === "item" ? entry.label : entry.kind))).toEqual([
      "Selection",
      "This paragraph",
      "Whole document",
      "separator",
      "To: German…",
    ]);
    // nothing selected in the editor
    expect(itemsOf(entries).map((entry) => entry.disabled === true)).toEqual([
      true,
      false,
      false,
      false,
    ]);
    vi.spyOn(editing, "scope").mockReturnValue({ from: 0, to: 5 });
    expect(item(registry, "Selection").disabled).toBe(false);
  });

  it("each scope of the menu starts its run, into the language chosen last", async () => {
    const registry = await editingWith("de");
    const runOver = vi.spyOn(translation, "runOver").mockResolvedValue();
    vi.spyOn(editing, "scope").mockReturnValue({ from: 0, to: 5 });
    for (const label of ["Selection", "This paragraph", "Whole document"])
      item(registry, label).run(false);
    expect(runOver.mock.calls).toEqual([["selection"], ["paragraph"], ["document"]]);
    expect(languageDialog.state.status).toBe("closed");
  });

  it("the menu names the language translated into; picking it opens the dialog, which only saves the choice", async () => {
    const registry = await editingWith(null);
    const runOver = vi.spyOn(translation, "runOver").mockResolvedValue();
    expect(itemsOf(menuOf(registry)).at(-1)?.label).toBe("Choose a language…");
    await translation.languages.choose("pt-BR");
    expect(itemsOf(menuOf(registry)).at(-1)?.label).toBe("To: Brazilian Portuguese…");
    item(registry, "To: Brazilian Portuguese…").run(false);
    expect(languageDialog.state).toMatchObject({
      status: "open",
      confirm: "translation.language.save",
      then: null,
    });
    languageDialog.confirm("de", null);
    expect(itemsOf(menuOf(registry)).at(-1)?.label).toBe("To: German…");
    expect(runOver).not.toHaveBeenCalled();
  });

  it("a first run asks for the language: Translate starts the run asked for, Cancel starts nothing", async () => {
    const registry = await editingWith(null);
    const runOver = vi.spyOn(translation, "runOver").mockResolvedValue();
    const runHere = vi.spyOn(translation, "runHere").mockResolvedValue();
    // the menu
    item(registry, "This paragraph").run(false);
    expect(languageDialog.state).toMatchObject({ status: "open", confirm: "translation.action" });
    expect(t("translation.action")).toBe("Translate");
    languageDialog.close();
    expect(runOver).not.toHaveBeenCalled();
    expect(translation.languages.target).toBeNull();
    item(registry, "Whole document").run(false);
    languageDialog.confirm("de", null);
    expect(languageDialog.state.status).toBe("closed");
    expect(translation.languages.target).toBe("de");
    expect(runOver.mock.calls).toEqual([["document"]]);
    // the commands (the key runs the first)
    translation.languages.reset();
    void command(registry, "translation.here").run();
    void command(registry, "translation.document").run();
    expect(runHere).not.toHaveBeenCalled();
    expect(languageDialog.state.status).toBe("open");
    languageDialog.confirm("ja", null);
    expect(runHere).toHaveBeenCalledOnce();
    expect(runOver.mock.calls).toEqual([["document"]]);
    // a language is there: they run at once
    void command(registry, "translation.here").run();
    void command(registry, "translation.document").run();
    expect(languageDialog.state.status).toBe("closed");
    expect(runHere).toHaveBeenCalledTimes(2);
    expect(runOver.mock.calls).toEqual([["document"], ["document"]]);
  });

  it("stepping through the translations is offered only while they are reviewed", async () => {
    const registry = await editingWith("de");
    const go = vi.spyOn(translation, "go").mockImplementation(() => {});
    const steps = ["translation.next", "translation.prev"].map((id) => command(registry, id));
    expect(steps.map((step) => t(step.label))).toEqual([
      "Next translation",
      "Previous translation",
    ]);
    expect(steps.map((step) => step.when?.())).toEqual([false, false]);
    // a run going on with nothing found yet: nothing to step through
    translation.state = { ...reviewOf(), status: { kind: "loading", progress: 0 } };
    expect(steps.map((step) => step.when?.())).toEqual([false, false]);
    translation.state = reviewOf(S1, S2);
    expect(steps.map((step) => step.when?.())).toEqual([true, true]);
    for (const step of steps) void step.run();
    expect(go.mock.calls).toEqual([[1], [-1]]);
    // back in View: not offered
    editing.mode = "view";
    expect(steps.map((step) => step.when?.())).toEqual([false, false]);
  });

  it("the key while its review is open moves the keyboard to the bar: no new run, no dialog", async () => {
    const registry = await editingWith("de");
    const runOver = vi.spyOn(translation, "runOver");
    translation.state = reviewOf(S1, S2);
    const asked = translation.focusRequest;
    void command(registry, "translation.here").run();
    expect(translation.focusRequest).toBe(asked + 1);
    expect(runOver).not.toHaveBeenCalled();
    expect(languageDialog.state.status).toBe("closed");
  });
});

describe("one review at a time", () => {
  it("while another plugin's review is open nothing of translation runs: the menu says why, the commands are not offered", async () => {
    const registry = await editingWith("de", "correction");
    const runOver = vi.spyOn(translation, "runOver");
    vi.spyOn(editing, "scope").mockReturnValue({ from: 0, to: 5 });
    correction.state = reviewOf(S1);
    expect(translation.blocked).toBe(true);
    const items = itemsOf(menuOf(registry));
    expect(items.map((entry) => entry.disabled === true)).toEqual([true, true, true, false]);
    expect(items.map((entry) => entry.detail)).toEqual([
      "Finish the current review first",
      "Finish the current review first",
      "Finish the current review first",
      undefined,
    ]);
    expect(t("localAi.review.busy")).toBe("Finish the current review first");
    // the button stays, the commands (and with them the key) go
    expect(
      registry
        .of("editAction")
        .find((a) => a.owner === "translation")
        ?.when?.(),
    ).toBe(true);
    expect(command(registry, "translation.here").when?.()).toBe(false);
    expect(command(registry, "translation.document").when?.()).toBe(false);
    // the language may still be chosen
    expect(command(registry, "translation.language").when?.()).toBe(true);
    // the other review closed: all of it is back
    correction.state = { kind: "closed" };
    expect(itemsOf(menuOf(registry)).map((entry) => entry.disabled === true)).toEqual([
      false,
      false,
      false,
      false,
    ]);
    expect(itemsOf(menuOf(registry)).every((entry) => entry.detail === undefined)).toBe(true);
    expect(command(registry, "translation.here").when?.()).toBe(true);
    expect(runOver).not.toHaveBeenCalled();
  });

  it("while translation's review is open correction's entries say the same and its own stay", async () => {
    const registry = await editingWith("de", "correction");
    await vi.waitFor(() => expect(correction.available).toBe(true));
    vi.spyOn(editing, "scope").mockReturnValue({ from: 0, to: 5 });
    translation.state = reviewOf(S1);
    expect(correction.blocked).toBe(true);
    const theirs = itemsOf(menuOf(registry, "correction.menu"));
    expect(theirs.map((entry) => entry.disabled === true)).toEqual([true, true, true]);
    expect(theirs.map((entry) => entry.detail)).toEqual(
      Array(3).fill("Finish the current review first"),
    );
    expect(command(registry, "correction.correct").when?.()).toBe(false);
    expect(command(registry, "correction.correctDocument").when?.()).toBe(false);
    // its own review does not hold translation back: a new run replaces it
    expect(itemsOf(menuOf(registry)).map((entry) => entry.disabled === true)).toEqual([
      false,
      false,
      false,
      false,
    ]);
    expect(command(registry, "translation.here").when?.()).toBe(true);
  });
});
