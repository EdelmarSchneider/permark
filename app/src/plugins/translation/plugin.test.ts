// @vitest-environment jsdom
// Plugin `translation` switched on | off: the review follows the model of job "translation" and the remembered
// languages, and nothing is left after; dormant while no model is assigned to the job. Its ways in while a document
// is edited: the button with the mode switch and its menu, the commands and the key; a first run asks for the
// language; none of them runs while another plugin's review is open. "Translate to a new file…": where it is
// offered, what it asks first, what holds it and what it holds, its place in the status bar.
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
const { documents } = await import("../../core/doc/document.svelte");
const { translation } = await import("./store.svelte");
const { copy } = await import("./copy.svelte");
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
    locked: 0,
    total: suggestions.length,
  };
}

let host: InstanceType<typeof PluginHost> | null = null;

afterEach(() => {
  host?.disable("translation");
  host?.disable("correction");
  host?.disable("local-ai");
  host = null;
  mock.probe.translation.available = null;
  mock.probe.translation.prefs = { target: null, recent: [] };
  mock.probe.translation.stepMs = 300;
  mock.probe.translation.stopAll();
  editing.mode = "view";
  editing.kind = "text";
  correction.state = { kind: "closed" };
  translation.state = { kind: "closed" };
  copy.state = { kind: "idle" };
  languageDialog.close();
  vi.restoreAllMocks();
});

/** Plugins that may be on beside it. */
type Other = "correction" | "local-ai";

/** The plugin on (after `first`, when given), in a window whose model for the job is there. */
async function on(...first: Other[]) {
  const registry = new ContributionRegistry();
  host = new PluginHost(registry, pluginLoaders());
  for (const id of first) await host.enable(id);
  await host.enable("translation");
  return { registry, host };
}

/** The plugin on, a model on its job, the open document edited, `target` the language chosen last. */
async function editingWith(target: string | null, ...first: Other[]) {
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
    // not Mod+Alt+t: on Linux desktops that key opens a terminal before the app sees it
    expect(own.flatMap((c) => (c.kind === "shortcut" ? [`${c.command} ${c.keys}`] : []))).toEqual([
      "translation.here Mod+Alt+r",
    ]);
    expect(own.flatMap((c) => (c.kind === "command" ? [c.id] : [])).sort()).toEqual([
      "translation.document",
      "translation.here",
      "translation.language",
      "translation.newFile",
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

  it("stepping through the suggestions: one pair of commands and keys, for whichever review is open", async () => {
    const registry = await editingWith("de", "local-ai", "correction");
    const go = vi.spyOn(translation, "go").mockImplementation(() => {});
    const goCorrection = vi.spyOn(correction, "go").mockImplementation(() => {});
    const steps = ["local-ai.nextSuggestion", "local-ai.prevSuggestion"].map((id) =>
      command(registry, id),
    );
    // neither review plugin has its own: two commands on one key would fight over it
    expect(steps.map((step) => step.owner)).toEqual(["local-ai", "local-ai"]);
    expect(
      registry
        .of("shortcut")
        .filter((s) => s.keys.endsWith("F7"))
        .map((s) => `${s.command} ${s.keys}`),
    ).toEqual(["local-ai.nextSuggestion F7", "local-ai.prevSuggestion Shift+F7"]);
    expect(steps.map((step) => t(step.label))).toEqual(["Next suggestion", "Previous suggestion"]);
    const offered = (): (boolean | undefined)[] => steps.map((step) => step.when?.());
    expect(offered()).toEqual([false, false]);
    // a run going on with nothing found yet: nothing to step through
    translation.state = { ...reviewOf(), status: { kind: "loading", progress: 0 } };
    expect(offered()).toEqual([false, false]);
    translation.state = reviewOf(S1, S2);
    expect(offered()).toEqual([true, true]);
    for (const step of steps) void step.run();
    expect(go.mock.calls).toEqual([[1], [-1]]);
    expect(goCorrection).not.toHaveBeenCalled();
    // correction's review open instead: the same commands step through it
    translation.state = { kind: "closed" };
    expect(offered()).toEqual([false, false]);
    correction.state = reviewOf(S1, S2);
    expect(offered()).toEqual([true, true]);
    for (const step of steps) void step.run();
    expect(goCorrection.mock.calls).toEqual([[1], [-1]]);
    expect(go).toHaveBeenCalledTimes(2);
    // back in View: not offered
    editing.mode = "view";
    expect(offered()).toEqual([false, false]);
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

describe("translate to a new file", () => {
  const NEW_FILE = "Translate to a new file…";
  const RUNNING = "A translation to a new file is running";
  /** A copy on its way. */
  const going: typeof copy.state = {
    kind: "running",
    file: "guide.md",
    status: { kind: "loading", progress: 0 },
  };
  const labels = (entries: MenuEntry[]): string[] =>
    entries.map((entry) => (entry.kind === "item" ? entry.label : entry.kind));

  /** The plugin on and `path` open in the editor as a Markdown document. */
  async function editingMarkdown(path: string, ...first: Other[]) {
    const registry = await editingWith("de", ...first);
    editing.kind = "markdown";
    vi.spyOn(editing, "path", "get").mockReturnValue(path);
    vi.spyOn(documents, "path", "get").mockReturnValue(path);
    return registry;
  }

  it("is in the menu after the scopes and a command, while a Markdown document is edited", async () => {
    const registry = await editingWith("de");
    const newFile = command(registry, "translation.newFile");
    expect(t(newFile.label)).toBe(NEW_FILE);
    // a text that is not Markdown: the review's entries alone
    expect(labels(menuOf(registry))).not.toContain(NEW_FILE);
    expect(newFile.when?.()).toBe(false);
    editing.kind = "markdown";
    expect(labels(menuOf(registry))).toEqual([
      "Selection",
      "This paragraph",
      "Whole document",
      "separator",
      NEW_FILE,
      "separator",
      "To: German…",
    ]);
    for (const mode of ["edit", "source"] as const) {
      editing.mode = mode;
      expect(newFile.when?.(), mode).toBe(true);
    }
    editing.mode = "view";
    expect(newFile.when?.()).toBe(false);
  });

  it("always asks for the language first, naming the file it will make; Translate starts it, Cancel nothing", async () => {
    const registry = await editingMarkdown("docs/readme.en.md");
    const run = vi.spyOn(copy, "run").mockResolvedValue();
    item(registry, NEW_FILE).run(false);
    // a language was chosen before: asked all the same
    const asked = languageDialog.state;
    if (asked.status !== "open") throw new Error("no dialog");
    expect(asked.confirm).toBe("translation.action");
    const note = (tag: string): string | null =>
      typeof asked.note === "function" ? asked.note(tag) : asked.note;
    expect(note("pt-BR")).toBe("Creates readme.pt-BR.md next to this file.");
    expect(note("ja")).toBe("Creates readme.ja.md next to this file.");
    languageDialog.close();
    expect(run).not.toHaveBeenCalled();
    void command(registry, "translation.newFile").run();
    expect(languageDialog.state.status).toBe("open");
    languageDialog.confirm("ja", null);
    expect(languageDialog.state.status).toBe("closed");
    expect(translation.languages.target).toBe("ja");
    expect(run).toHaveBeenCalledOnce();
  });

  it("is off while a translation review is open or a copy runs, and says why; correction's review does not hold it", async () => {
    const registry = await editingMarkdown("docs/guide.md", "correction");
    const run = vi.spyOn(copy, "run").mockResolvedValue();
    const newFile = command(registry, "translation.newFile");
    const entry = (): Item => item(registry, NEW_FILE);
    expect(entry().disabled).not.toBe(true);
    expect(entry().detail).toBeUndefined();
    expect(newFile.when?.()).toBe(true);
    // suggestions to go through
    translation.state = reviewOf(S1);
    expect(entry()).toMatchObject({ disabled: true, detail: "Finish the current review first" });
    expect(newFile.when?.()).toBe(false);
    // a run going on, nothing found yet
    translation.state = { ...reviewOf(), status: { kind: "loading", progress: 0 } };
    expect(entry().disabled).toBe(true);
    expect(newFile.when?.()).toBe(false);
    void newFile.run();
    entry().run(false);
    expect(languageDialog.state.status).toBe("closed");
    translation.state = { kind: "closed" };
    correction.state = reviewOf(S1);
    expect(entry().disabled).not.toBe(true);
    expect(newFile.when?.()).toBe(true);
    correction.state = { kind: "closed" };
    copy.state = going;
    expect(entry()).toMatchObject({ disabled: true, detail: RUNNING });
    expect(newFile.when?.()).toBe(false);
    void newFile.run();
    entry().run(false);
    expect(languageDialog.state.status).toBe("closed");
    expect(run).not.toHaveBeenCalled();
    expect(t("translation.copy.busy")).toBe(RUNNING);
  });

  it("a review that opens while the language is asked for: the copy does not start", async () => {
    const registry = await editingMarkdown("docs/guide.md");
    const run = vi.spyOn(copy, "run").mockResolvedValue();
    item(registry, NEW_FILE).run(false);
    translation.state = reviewOf(S1);
    languageDialog.confirm("ja", null);
    expect(run).not.toHaveBeenCalled();
  });

  it("while a copy runs the review's entries are off and say why; the language and correction stay", async () => {
    const registry = await editingMarkdown("docs/guide.md", "correction");
    await vi.waitFor(() => expect(correction.available).toBe(true));
    const runOver = vi.spyOn(translation, "runOver");
    const runHere = vi.spyOn(translation, "runHere");
    vi.spyOn(editing, "scope").mockReturnValue({ from: 0, to: 5 });
    copy.state = going;
    const items = itemsOf(menuOf(registry));
    expect(items.map((entry) => entry.disabled === true)).toEqual([true, true, true, true, false]);
    expect(items.map((entry) => entry.detail)).toEqual([
      RUNNING,
      RUNNING,
      RUNNING,
      RUNNING,
      undefined,
    ]);
    // the commands (and with them the key) go; whatever still asks starts nothing
    expect(command(registry, "translation.here").when?.()).toBe(false);
    expect(command(registry, "translation.document").when?.()).toBe(false);
    for (const label of ["Selection", "This paragraph", "Whole document"])
      item(registry, label).run(false);
    void command(registry, "translation.here").run();
    void command(registry, "translation.document").run();
    expect(runOver).not.toHaveBeenCalled();
    expect(runHere).not.toHaveBeenCalled();
    expect(command(registry, "translation.language").when?.()).toBe(true);
    // a copy changes nothing in the editor: correction goes on
    expect(
      itemsOf(menuOf(registry, "correction.menu")).map((entry) => entry.disabled === true),
    ).toEqual([false, false, false]);
    expect(command(registry, "correction.correct").when?.()).toBe(true);
    // the copy over: all of it is back
    copy.state = { kind: "idle" };
    expect(itemsOf(menuOf(registry)).some((entry) => entry.disabled === true)).toBe(false);
    expect(itemsOf(menuOf(registry)).every((entry) => entry.detail === undefined)).toBe(true);
    expect(command(registry, "translation.here").when?.()).toBe(true);
    expect(command(registry, "translation.document").when?.()).toBe(true);
  });

  it("a first run's language confirmed after a copy started: the review does not run", async () => {
    const registry = await editingWith(null);
    const runOver = vi.spyOn(translation, "runOver").mockResolvedValue();
    item(registry, "Whole document").run(false);
    expect(languageDialog.state.status).toBe("open");
    copy.state = going;
    languageDialog.confirm("de", null);
    expect(runOver).not.toHaveBeenCalled();
  });

  it("its place in the status bar shows only a copy that runs, with a Cancel that names the file", async () => {
    const { registry } = await on();
    const place = registry.of("statusItem").find((c) => c.id === "translation.copy");
    const { default: Place } = (await place?.view()) ?? { default: null };
    if (Place === null) throw new Error("no place");
    const target = document.createElement("div");
    document.body.append(target);
    const app = mount(Place, { target });
    flushSync();
    expect(target.textContent?.trim()).toBe("");
    copy.state = going;
    flushSync();
    expect(target.textContent).toContain("Getting the model ready…");
    expect(target.querySelector("button")?.getAttribute("aria-label")).toBe(
      "Cancel translation of guide.md",
    );
    copy.state = { kind: "idle" };
    flushSync();
    expect(target.textContent?.trim()).toBe("");
    await unmount(app);
    target.remove();
  });

  it("switched off mid-run: the run is stopped and nothing is left", async () => {
    mock.probe.translation.stepMs = 1e9;
    const registry = await editingMarkdown("docs/guide.md");
    vi.spyOn(editing, "current").mockReturnValue("A text to translate.\n");
    const files = [...mock.probe.project.files.keys()];
    await copy.run();
    const run = mock.probe.translation.runs.at(-1)?.run;
    expect(run).toBeDefined();
    expect(copy.running).toBe(true);
    expect(host?.disable("translation"), "contributions left after deactivate").toBe(0);
    expect(registry.ownedBy("translation")).toHaveLength(0);
    expect(copy.running).toBe(false);
    expect(mock.probe.translation.calls).toContain(`cancel ${run ?? ""}`);
    expect([...mock.probe.project.files.keys()]).toEqual(files);
  });
});
