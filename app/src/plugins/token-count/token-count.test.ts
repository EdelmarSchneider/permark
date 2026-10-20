// @vitest-environment jsdom
// The token count against the mock backend: what the plugin adds and removes, the store, the chooser, the status item
// with its popover, and the outline column.
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceView } from "../../api";
import { MOCK_FOLDER } from "../../api/mock";
import { standIn } from "../../api/mockTokens";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});
Element.prototype.scrollIntoView = () => {};

const { api } = await import("../../api");
const { ContributionRegistry, PluginHost, pluginLoaders } =
  await import("../../core/plugins/registry.svelte");
const { documents } = await import("../../core/doc/document.svelte");
const { windowStore } = await import("../../core/window.svelte");
const { projectSettings } = await import("../../core/shell/settings/stores.svelte");
const { tokens } = await import("./store.svelte");
const { sections } = await import("./sections.svelte");
const { estimate } = await import("./estimate");
const { default: TokenChooser } = await import("./TokenChooser.svelte");
const { default: TokenChoice } = await import("./TokenChoice.svelte");
const { default: TokenStatus } = await import("./TokenStatus.svelte");
const { default: OutlineTokens } = await import("./OutlineTokens.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const probe = mock.probe.tokens;

type Shown = Extract<WorkspaceView, { kind: "project" }>;

let target: HTMLElement;
let component: Record<string, unknown> | null = null;
let stop: (() => void) | null = null;
let workspace: Shown;
let started = false;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 3000, interval: 10 });
}

const text = (): string => (target.textContent ?? "").replace(/\s+/g, " ").trim();

function byText<T extends HTMLElement = HTMLElement>(selector: string, wanted: string): T {
  const found = Array.from(target.querySelectorAll<T>(selector)).find((el) =>
    el.textContent?.replace(/\s+/g, " ").trim().includes(wanted),
  );
  if (!found) throw new Error(`no ${selector} with "${wanted}" in: ${text()}`);
  return found;
}

async function choose(...ids: string[]): Promise<void> {
  await windowStore.changeSetting({ key: "tokenizers", value: ids });
  await until(() => windowStore.tokenizers.join() === ids.join(), "the choice reaches the window");
}

beforeEach(async () => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  if (!started) {
    await windowStore.start();
    started = true;
  }
  mock.probe.shell.forgetInit();
  await mock.openPath(MOCK_FOLDER, "thisTab");
  await until(() => windowStore.view.state === "showing", "the project shows");
  const view = windowStore.view;
  if (view.state !== "showing" || view.workspace.kind !== "project") throw new Error("no project");
  workspace = view.workspace;
  probe.steps = 2;
  probe.failNext = null;
  probe.calls.length = 0;
  probe.counted.length = 0;
  probe.setVocab("o200k", { kind: "absent" });
  probe.setVocab("cl100k", { kind: "absent" });
  await choose();
  stop = tokens.start();
  await until(() => tokens.state !== null, "the state is read");
  await tokens.setOutline(false);
});

afterEach(async () => {
  stop?.();
  stop = null;
  if (component) await unmount(component);
  component = null;
  documents.setWorkspace(null);
  sections.clear();
  await windowStore.changeSetting({ key: "tokenizers", value: [] });
});

describe("what the plugin adds", () => {
  it("adds its parts when switched on and leaves nothing when switched off", async () => {
    stop?.();
    stop = null;
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    await host.enable("token-count");
    expect(host.state("token-count")).toBe("on");
    expect([...new Set(registry.ownedBy("token-count").map((c) => c.kind))].sort()).toEqual([
      "command",
      "outlineColumn",
      "projectSection",
      "settingsSection",
      "statusItem",
      "treeBadge",
      "treeMenu",
    ]);
    await until(() => tokens.state !== null, "the store started with the plugin");
    expect(host.disable("token-count"), "contributions left after deactivate").toBe(0);
    expect(registry.items).toHaveLength(0);
    expect(tokens.state).toBeNull();
    expect(tokens.popover).toBe(false);
  });
});

describe("the store", () => {
  it("follows the vocabularies the backend announces and runs the person's actions", async () => {
    expect(tokens.state?.vocab.o200k.state.kind).toBe("absent");
    await tokens.download("o200k");
    await until(() => tokens.state?.vocab.o200k.state.kind === "downloading", "it starts");
    await until(() => tokens.state?.vocab.o200k.state.kind === "ready", "it finishes");
    expect(probe.calls).toContain("download o200k");
    await tokens.remove("o200k");
    expect(tokens.state?.vocab.o200k.state.kind).toBe("absent");
    // a download can be cancelled, and a failure says why
    await tokens.download("cl100k");
    await tokens.cancel("cl100k");
    expect(tokens.state?.vocab.cl100k.state.kind).toBe("absent");
    probe.failNext = { kind: "offline" };
    await tokens.download("cl100k");
    await until(() => tokens.state?.vocab.cl100k.state.kind === "failed", "it fails");
    expect(tokens.state?.vocab.cl100k.state).toEqual({
      kind: "failed",
      reason: { kind: "offline" },
    });
  });

  it("an estimate is ready at once, an exact tokenizer only with its vocabulary", async () => {
    const [o200k, claude] = [tokens.vocabOf("o200k"), tokens.vocabOf("claude")];
    expect(o200k?.state.kind).toBe("absent");
    expect(claude).toBeNull();
    await choose("claude", "o200k");
    const [first, second] = tokens.chosen;
    expect(first && tokens.ready(first)).toBe(true);
    expect(second && tokens.ready(second)).toBe(false);
    probe.setVocab("o200k", { kind: "ready" });
    await until(() => (second ? tokens.ready(second) : false), "ready after the download");
  });

  it("the app's choice, and the project's own over it once PerMark is set up there", async () => {
    await choose("o200k", "claude");
    expect(tokens.chosen.map((i) => i.id)).toEqual(["o200k", "claude"]);
    // no .permark/: the project cannot keep a choice
    await projectSettings.load(mock);
    expect(tokens.projectCanChoose).toBe(false);
    expect(await tokens.choose("project", ["gemini"])).toBe(false);
    expect(tokens.chosen.map((i) => i.id)).toEqual(["o200k", "claude"]);
    await api.projectInit(workspace.root);
    await projectSettings.load(mock);
    expect(tokens.projectCanChoose).toBe(true);
    expect(await tokens.choose("project", ["gemini"])).toBe(true);
    expect(tokens.chosen.map((i) => i.id)).toEqual(["gemini"]);
    // clearing the project's list gives the app's choice back
    expect(await tokens.choose("project", [])).toBe(true);
    expect(tokens.chosen.map((i) => i.id)).toEqual(["o200k", "claude"]);
  });

  it("the outline switch is kept by the backend", async () => {
    expect(tokens.state?.outline).toBe(false);
    await tokens.setOutline(true);
    expect(tokens.state?.outline).toBe(true);
  });
});

describe("the chooser", () => {
  function render(ids: string[], onchange = vi.fn(), removable = false): ReturnType<typeof vi.fn> {
    component = mount(TokenChooser, { target, props: { ids, onchange, removable } });
    flushSync();
    return onchange;
  }

  it("marks every tokenizer as exact or approximate and says why", () => {
    render(["o200k"]);
    const rows = Array.from(target.querySelectorAll("li"));
    expect(rows).toHaveLength(5);
    const badges = rows.map((row) => row.querySelector(".badge")?.textContent?.trim());
    expect(badges).toEqual(["Exact", "Exact", "Approximate", "Approximate", "Approximate"]);
    expect(text()).toContain("Claude's tokenizer is not public");
    expect(text()).toContain("Same count as the model's own tokenizer.");
    expect(text()).toContain("GPT-4o, GPT-4.1, o-series, GPT-5");
  });

  it("choosing an exact tokenizer starts no download: the person presses Download", async () => {
    const onchange = render([]);
    byText<HTMLInputElement>("li", "GPT-4o").querySelector("input")?.click();
    expect(onchange).toHaveBeenCalledWith(["o200k"]);
    expect(probe.calls).toEqual([]);
    byText<HTMLButtonElement>("button", "Download 3.6 MB").click();
    await until(() => probe.calls.includes("download o200k"), "the download is asked for");
  });

  it("at most three can be chosen, and the first is the primary", () => {
    const onchange = render(["claude", "gemini", "o200k"]);
    const boxes = Array.from(target.querySelectorAll<HTMLInputElement>("input[type=checkbox]"));
    expect(boxes.filter((box) => box.disabled)).toHaveLength(2);
    expect(text()).toContain("Primary");
    byText<HTMLButtonElement>("button", "Make primary").click();
    // the first row that is not primary is GPT-4o
    expect(onchange).toHaveBeenCalledWith(["o200k", "claude", "gemini"]);
  });

  it("shows the progress of a download, a failure with its reason and a way to retry", async () => {
    render(["o200k", "cl100k"]);
    probe.setVocab("o200k", { kind: "downloading", done: 1_800_000, total: 3_600_000 });
    await until(() => text().includes("Downloading… 50 %"), "progress");
    expect(text()).toContain("Cancel");
    probe.setVocab("o200k", { kind: "failed", reason: { kind: "http", status: 503 } });
    await until(() => text().includes("(503)"), "the status");
    probe.setVocab("cl100k", { kind: "failed", reason: { kind: "hash" } });
    await until(() => text().includes("not the official one"), "the hash");
    expect(target.querySelectorAll("[role=alert]")).toHaveLength(2);
    byText<HTMLButtonElement>("button", "Try again").click();
    await until(() => probe.calls.includes("download o200k"), "retry");
  });

  it("a downloaded vocabulary can be removed where removing is offered", async () => {
    probe.setVocab("o200k", { kind: "ready" });
    render(["o200k"], vi.fn(), true);
    await until(() => text().includes("Remove (3.6 MB)"), "remove offered");
    byText<HTMLButtonElement>("button", "Remove (3.6 MB)").click();
    await until(() => probe.calls.includes("removeVocab o200k"), "removed");
  });
});

describe("the choice with its scope", () => {
  it("offers a choice for this project only once PerMark is set up in it", async () => {
    await projectSettings.load(mock);
    component = mount(TokenChoice, { target, props: { mode: "both" } });
    flushSync();
    expect(text()).not.toContain("A choice for this project only");
    await unmount(component);
    component = null;

    await api.projectInit(workspace.root);
    await projectSettings.load(mock);
    await choose("claude");
    component = mount(TokenChoice, { target, props: { mode: "both" } });
    flushSync();
    const own = byText<HTMLLabelElement>("label", "A choice for this project only").querySelector(
      "input",
    ) as HTMLInputElement;
    expect(own.checked).toBe(false);
    // the project keeps the choice in effect, then gives it back
    own.click();
    await until(() => tokens.projectList.join() === "claude", "the project keeps the choice");
    flushSync();
    expect(own.checked).toBe(true);
    own.click();
    await until(() => tokens.projectList.length === 0, "the project uses the app's again");
  });

  it("knows the project's own choice without the settings page having been opened", async () => {
    await api.projectInit(workspace.root);
    await api.projectConfigSet({ key: "tokenizers", value: ["gemini"] });
    projectSettings.state = { status: "idle" };
    stop?.();
    stop = tokens.start();
    await until(() => tokens.projectList.join() === "gemini", "the project's list is read");
    expect(tokens.projectCanChoose).toBe(true);
    expect(tokens.chosen.map((info) => info.id)).toEqual(["gemini"]);
    component = mount(TokenChoice, { target, props: { mode: "both" } });
    flushSync();
    const own = byText<HTMLLabelElement>("label", "A choice for this project only").querySelector(
      "input",
    ) as HTMLInputElement;
    expect(own.checked).toBe(true);
    // the chooser edits the project's list: unchecking Gemini and choosing Claude writes the project file
    byText<HTMLInputElement>("li", "Gemini").querySelector("input")?.click();
    await until(() => tokens.projectList.length === 0, "the project's list is emptied");
  });

  it("the project card says so when the project has no .permark/", async () => {
    await projectSettings.load(mock);
    component = mount(TokenChoice, { target, props: { mode: "project" } });
    flushSync();
    expect(text()).toContain("Initialize PerMark in this project");
  });
});

describe("the status item", () => {
  async function show(path: string): Promise<void> {
    documents.setWorkspace(workspace);
    await documents.show(path);
    await until(() => documents.view !== null, "the document shows");
    component = mount(TokenStatus, { target });
    flushSync();
  }

  const bar = (): string => (target.querySelector(".item .label")?.textContent ?? "").trim();

  it("is a chip to choose while nothing is chosen, and opens the choice", async () => {
    await show("README.md");
    expect(bar()).toBe("Choose a tokenizer");
    target.querySelector<HTMLButtonElement>(".item")?.click();
    await until(() => tokens.popover, "the popover opens");
    flushSync();
    expect(text()).toContain("Change tokenizers");
    expect(text()).toContain("Approximate");
    expect(text()).toContain("Exact");
  });

  it("marks an estimate with ≈ and names the tokenizer", async () => {
    await choose("claude");
    await show("README.md");
    const expected = estimate("claude", "# README.md\n");
    await until(() => bar().startsWith("≈"), "the number shows");
    expect(bar()).toBe(`≈ ${expected} ${expected === 1 ? "token" : "tokens"} · Claude`);
    expect(target.querySelector(".item")?.getAttribute("title")).toContain("approx.");
  });

  it("an exact tokenizer without its vocabulary offers the download, never a number", async () => {
    await choose("o200k");
    await show("README.md");
    await until(
      () => bar() === "Download GPT-4o vocabulary",
      `the download chip: [${bar()}] ${text()}`,
    );
    expect(probe.calls).toEqual([]);
    probe.setVocab("o200k", { kind: "downloading", done: 900_000, total: 3_600_000 });
    await until(() => bar() === "GPT-4o vocabulary 25 %", `progress: ${bar()} / ${text()}`);
    probe.setVocab("o200k", { kind: "failed", reason: { kind: "offline" } });
    await until(() => bar() === "GPT-4o unavailable", "unavailable");
    expect(probe.counted).toEqual([]);
  });

  it("shows the exact number without ≈ once the backend has counted", async () => {
    await choose("o200k");
    probe.setVocab("o200k", { kind: "ready" });
    await show("README.md");
    const exact = standIn("o200k", "# README.md\n");
    await until(() => bar() === `${exact} ${exact === 1 ? "token" : "tokens"} · GPT-4o`, "exact");
    expect(probe.counted).toContain("# README.md\n");
    expect(target.querySelector(".item")?.getAttribute("title")).not.toContain("approx.");
  });

  it("lists every tokenizer chosen in the popover, each with its badge and figures", async () => {
    await choose("claude", "chars4");
    await show("README.md");
    await until(() => bar().startsWith("≈"), "the number shows");
    target.querySelector<HTMLButtonElement>(".item")?.click();
    await until(() => tokens.popover, "open");
    flushSync();
    const heads = Array.from(target.querySelectorAll(".tokenizer .head")).map((h) =>
      (h.textContent ?? "").replace(/\s+/g, " ").trim(),
    );
    expect(heads).toEqual(["Claude Approximate Primary", "chars ÷ 4 Approximate"]);
    expect(text()).toContain("Document");
    expect(text()).toContain("A token is a piece of text a model reads");
    expect(text()).toContain("Show tokens in the outline");
  });

  it("closes on Escape", async () => {
    await choose("claude");
    await show("README.md");
    target.querySelector<HTMLButtonElement>(".item")?.click();
    await until(() => tokens.popover, "open");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await until(() => !tokens.popover, "closed");
  });
});

describe("the outline column", () => {
  it("has no header until the switch is on, and a tokenizer that can count", async () => {
    await choose("claude");
    expect(sections.header()).toBeNull();
    await tokens.setOutline(true);
    expect(sections.header()?.text).toBe("≈ Tokens");
    expect(sections.header()?.title).toContain("Claude");
    await choose();
    expect(sections.header()).toBeNull();
    await choose("o200k");
    expect(sections.header(), "exact without its vocabulary").toBeNull();
    probe.setVocab("o200k", { kind: "ready" });
    await until(() => sections.header() !== null, "ready");
    expect(sections.header()?.text).toBe("Tokens");
  });

  it("works out the tokens of each section for the page that shows", async () => {
    await choose("claude");
    await tokens.setOutline(true);
    documents.setWorkspace(workspace);
    await documents.show("docs/guide.md");
    await until(() => documents.view?.body.kind === "markdown", "the guide shows");
    const items = documents.view?.body.kind === "markdown" ? documents.view.body.outline : [];
    expect(items.length).toBeGreaterThan(1);
    component = mount(OutlineTokens, { target, props: { items } });
    flushSync();
    await until(() => sections.cell(0) !== null, "the first section is counted");
    for (let at = 0; at < items.length; at++)
      expect(sections.cell(at)?.text, `${at}`).toMatch(/^\d+$/);
    expect(sections.cell(items.length)).toBeNull();
    // the sections ask the backend once, for the primary tokenizer, with a range per heading
    expect(probe.counted.length).toBeGreaterThan(0);
  });
});
