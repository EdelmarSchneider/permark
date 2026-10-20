// @vitest-environment jsdom
// The folder count against the mock backend: the job from the person's request to the summary, the guard's questions,
// cancel, the badge and its stale mark, the progress in the status item, the totals counted by themselves, and what the
// plugin adds and removes.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FolderJob } from "../../api";
import { MOCK_FOLDER } from "../../api/mock";
import { STANDARD } from "../../api/mockTokenFolders";

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
const { tokens } = await import("./store.svelte");
const { folders, FolderStore: FolderStoreClass } = await import("./folders.svelte");
const { setExplorer } = await import("../../core/explorer/current");
const { default: FolderGuard } = await import("./FolderGuard.svelte");
const { default: FolderHost } = await import("./FolderHost.svelte");
const { default: FolderSummary } = await import("./FolderSummary.svelte");
const { default: TokenStatus } = await import("./TokenStatus.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const probe = mock.probe.tokens;
const counting = probe.folders;

let target: HTMLElement;
let component: Record<string, unknown> | null = null;
let stopTokens: (() => void) | null = null;
let stopFolders: (() => void) | null = null;
let started = false;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 3000, interval: 5 });
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

/** Asks for a count and waits until the job is over and its summary (if one is asked for) is up. */
async function count(folder: string): Promise<void> {
  await folders.count(folder);
  await until(() => folders.summary !== null, `the summary of ${folder}`);
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
  counting.reset();
  windowStore.notices = [];
  Object.assign(counting.thresholds, STANDARD);
  counting.stepMs = 3;
  counting.steps = 3;
  counting.calls.length = 0;
  probe.setVocab("o200k", { kind: "absent" });
  probe.setVocab("cl100k", { kind: "absent" });
  await choose("chars4");
  stopTokens = tokens.start();
  await until(() => tokens.state !== null, "the state is read");
  await tokens.setFolderTotals(false);
  stopFolders = folders.start();
  await folders.load(null);
  await folders.load(MOCK_FOLDER);
});

afterEach(async () => {
  stopFolders?.();
  stopFolders = null;
  stopTokens?.();
  stopTokens = null;
  if (component) await unmount(component);
  component = null;
  documents.setWorkspace(null);
  await windowStore.changeSetting({ key: "tokenizers", value: [] });
});

describe("a count from request to summary", () => {
  it("scans, counts in steps, ends in a summary and leaves a badge on the folder", async () => {
    const seen: string[] = [];
    const watching = setInterval(() => {
      if (seen.at(-1) !== folders.job.state) seen.push(folders.job.state);
    }, 1);
    await count("docs");
    clearInterval(watching);
    expect(seen).toContain("counting");
    const summary = folders.summary;
    expect(summary?.folder).toBe("docs");
    expect(summary?.tokenizer).toBe("chars4");
    expect(summary?.approx).toBe(true);
    // the guide is counted, the logo is a picture by its name
    expect(summary?.counted).toBe(1);
    expect(summary?.skippedBinary).toBe(1);
    expect(summary?.skippedList[0]).toEqual({ path: "docs/img/logo.png", reason: "binary" });
    expect(summary?.top[0]?.path).toBe("docs/guide.md");
    await until(() => folders.job.state === "idle", "the job is dismissed");
    // the badge: quiet, ≈ because chars÷4 is an estimate
    const badge = folders.badge("docs", true);
    expect(badge?.text).toMatch(/^≈\d+$/);
    expect(badge?.tone).toBe("muted");
    expect(badge?.tip).toContain("chars ÷ 4");
    expect(folders.badge("docs/guide.md", false)).toBeNull();
    expect(folders.badge("notes", true)).toBeNull();
    expect(counting.calls[0]).toBe("folderStart docs chars4");
  });

  it("nothing chosen: the chooser opens instead and no job starts", async () => {
    await choose();
    await folders.count("docs");
    expect(tokens.popover).toBe(true);
    expect(tokens.choosing).toBe(true);
    expect(counting.calls).toEqual([]);
    tokens.popover = false;
    tokens.choosing = false;
  });

  it("an exact tokenizer without its vocabulary is an estimate, with it an exact number", async () => {
    await choose("o200k");
    await count("notes");
    expect(folders.summary?.approx).toBe(true);
    expect(folders.badge("notes", true)?.text.startsWith("≈")).toBe(true);
    folders.closeSummary();
    probe.setVocab("o200k", { kind: "ready" });
    await count("notes");
    expect(folders.summary?.approx).toBe(false);
    expect(folders.badge("notes", true)?.text.startsWith("≈")).toBe(false);
    // the dialog says which
    component = mount(FolderSummary, { target });
    flushSync();
    expect(text()).toContain("Exact");
    expect(text()).toContain("GPT-4o");
  });

  it("a count is refused while another works, with a word", async () => {
    counting.stepMs = 30;
    await folders.count("");
    await folders.count("docs");
    expect(windowStore.notices.some((n) => n.kind === "note")).toBe(true);
    await until(() => folders.summary !== null, "the first count ends");
  });

  it("the summary dialog tells everything, copies itself and counts again", async () => {
    await count("");
    component = mount(FolderSummary, { target });
    flushSync();
    expect(text()).toContain("Tokens in the project");
    expect(text()).toContain("Approximate");
    expect(text()).toContain("files counted");
    expect(text()).toContain("By file type");
    expect(text()).toContain("Largest files");
    expect(text()).toContain("not text");
    // the skipped files are listed, folded
    expect(target.querySelector("details")).not.toBeNull();
    const written: string[] = [];
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: (s: string) => (written.push(s), Promise.resolve()) },
      configurable: true,
    });
    byText<HTMLButtonElement>("button", "Copy summary").click();
    await until(() => written.length === 1, "copied");
    expect(written[0]).toContain("the project:");
    expect(written[0]).toContain("By file type:");
    // again: the dialog goes, the count runs, the dialog comes back with the cached files counted
    byText<HTMLButtonElement>("button", "Count again").click();
    await until(() => folders.summary === null, "the dialog goes");
    await until(() => folders.summary !== null, "the second summary");
    expect(folders.summary?.cached).toBe(folders.summary?.counted);
    // Escape closes it
    flushSync();
    target
      .querySelector("[role=dialog]")
      ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await until(() => folders.summary === null, "closed");
  });
});

describe("the guard", () => {
  beforeEach(() => {
    // the mock project holds nine files
    counting.thresholds.confirmFiles = 2;
    counting.thresholds.refuseFiles = 50;
  });

  it("asks before a big folder, with the numbers and a time, and counts when agreed", async () => {
    component = mount(FolderGuard, { target });
    await folders.count("");
    await until(() => folders.job.state === "confirm", "the question");
    flushSync();
    expect(counting.results()).toEqual([]);
    expect(text()).toContain("Count the tokens in the project?");
    expect(text()).toMatch(/holds \d+ files/);
    expect(text()).toContain("about 1 second");
    expect(text()).toContain("Nothing in the folder is changed");
    byText<HTMLButtonElement>("button", "Count").click();
    await until(() => folders.summary !== null, "counted after the answer");
    expect(counting.calls).toContain("folderConfirm");
    flushSync();
    expect(target.querySelector("[role=dialog]")).toBeNull();
  });

  it("says no: nothing is counted and no badge appears", async () => {
    component = mount(FolderGuard, { target });
    await folders.count("");
    await until(() => folders.job.state === "confirm", "the question");
    flushSync();
    byText<HTMLButtonElement>("button", "Cancel").click();
    await until(() => folders.job.state === "idle", "back to idle");
    expect(counting.results()).toEqual([]);
    expect(folders.summary).toBeNull();
    flushSync();
    expect(target.querySelector("[role=dialog]")).toBeNull();
  });

  it("Escape is no too", async () => {
    component = mount(FolderGuard, { target });
    await folders.count("");
    await until(() => folders.job.state === "confirm", "the question");
    flushSync();
    target
      .querySelector("[role=dialog]")
      ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await until(() => folders.job.state === "idle", "back to idle");
  });

  it("refuses a folder that is too big, says why and offers only to close", async () => {
    counting.thresholds.confirmFiles = 100;
    counting.thresholds.refuseFiles = 3;
    component = mount(FolderGuard, { target });
    await folders.count("");
    await until(() => folders.job.state === "refused", "refused");
    flushSync();
    expect(text()).toContain("too big to count");
    expect(text()).toContain("more than 50,000 files");
    expect(text()).toContain("Count a subfolder instead.");
    expect(Array.from(target.querySelectorAll("button")).map((b) => b.textContent?.trim())).toEqual(
      ["Close"],
    );
    byText<HTMLButtonElement>("button", "Close").click();
    await until(() => folders.job.state === "idle", "dismissed");
    expect(counting.results()).toEqual([]);
  });
});

describe("an answer that comes after the event it predates", () => {
  it("does not take the state back: a thread that works fast has announced the next state already", async () => {
    // a call that is answered late: the backend asked the guard's question before the start was answered
    const events: { window: string; job: FolderJob }[] = [];
    let announce: ((e: { window: string; job: FolderJob }) => void) | null = null;
    const fake = {
      onAppPlugin: (event: string, handler: (e: { window: string; job: FolderJob }) => void) => {
        if (event === "token-count:folder") announce = handler;
        return Promise.resolve(() => {});
      },
    } as unknown as typeof api;
    const late = {
      call: async (method: string) => {
        if (method === "token-count.folderStart") {
          const confirm: FolderJob = {
            state: "confirm",
            folder: "docs",
            files: 5_000,
            bytes: 1,
            seconds: 3,
          };
          events.push({ window: windowStore.label, job: confirm });
          announce?.({ window: windowStore.label, job: confirm });
          // the answer says what the state was when the call returned: scanning
          return { ok: true, value: { state: "scanning", folder: "docs" } };
        }
        return { ok: true, value: { state: "idle" } };
      },
    } as unknown as ConstructorParameters<typeof FolderStoreClass>[1];
    const store = new FolderStoreClass(fake, late, tokens);
    const off = store.start();
    await store.count("docs");
    expect(events).toHaveLength(1);
    expect(store.job.state, "the question stays").toBe("confirm");
    off();
  });
});

describe("stopping", () => {
  it("a count stopped half way leaves no badge and no summary", async () => {
    counting.stepMs = 20;
    counting.steps = 20;
    await folders.count("");
    await until(() => folders.job.state === "counting", "counting");
    await folders.cancel();
    await until(() => folders.job.state === "idle", "stopped and dismissed");
    expect(folders.summary).toBeNull();
    expect(counting.results()).toEqual([]);
    expect(folders.badge("", true)).toBeNull();
    expect(folders.counted).toEqual([]);
  });
});

describe("the status item", () => {
  async function show(): Promise<void> {
    const view = windowStore.view;
    if (view.state !== "showing" || view.workspace.kind !== "project")
      throw new Error("no project");
    documents.setWorkspace(view.workspace);
    await documents.show("README.md");
    await until(() => documents.view !== null, "the document shows");
    component = mount(TokenStatus, { target });
    flushSync();
  }

  const bar = (): string => (target.querySelector(".item .label")?.textContent ?? "").trim();

  it("shows the progress while the folder is counted and stops it from the popover", async () => {
    counting.stepMs = 25;
    counting.steps = 20;
    await show();
    await folders.count("");
    await until(() => /^Counting \d+ \/ \d+ files$/.test(bar()), `the progress: ${bar()}`);
    target.querySelector<HTMLButtonElement>(".item")?.click();
    await until(() => tokens.popover, "the popover opens");
    flushSync();
    expect(target.querySelector("progress")).not.toBeNull();
    byText<HTMLButtonElement>("button", "Stop counting").click();
    await until(() => folders.job.state === "idle", "stopped");
    flushSync();
    expect(bar()).toMatch(/tokens/);
    tokens.popover = false;
  });

  it("lists the counted folders in the popover and the stale mark shows", async () => {
    await show();
    await count("docs");
    folders.closeSummary();
    counting.touch("docs/guide.md");
    await until(() => folders.isStale("docs"), "stale");
    expect(folders.badge("docs", true)?.text.endsWith("*")).toBe(true);
    expect(folders.badge("docs", true)?.tone).toBe("modified");
    expect(folders.badge("docs", true)?.title).toContain("Changed since it was counted");
    target.querySelector<HTMLButtonElement>(".item")?.click();
    await until(() => tokens.popover, "open");
    flushSync();
    expect(text()).toContain("Counted folders");
    byText<HTMLButtonElement>("button.link", "docs").click();
    await until(() => folders.summary !== null, "the summary opens from the list");
    expect(folders.summary?.folder).toBe("docs");
    tokens.popover = false;
  });
});

describe("the Explorer that comes after the plugin", () => {
  it("is heard: the folders it already has open are counted when the totals are on", async () => {
    await tokens.setFolderTotals(true);
    component = mount(FolderHost, { target });
    flushSync();
    // the tree is not there yet: nothing to count
    expect(counting.calls).toEqual([]);
    setExplorer({
      expanded: new Set(["notes"]),
      selectedFolder: () => null,
    } as unknown as Parameters<typeof setExplorer>[0]);
    await until(
      () => folders.counted.some((c) => c.folder === "notes"),
      "the open folder is counted",
    );
    expect(counting.calls.filter((c) => c.endsWith(" auto"))).toEqual([
      "folderStart notes chars4 auto",
    ]);
    setExplorer(null);
  });
});

describe("a change after the count", () => {
  it("marks the badge stale until the folder is counted again", async () => {
    await count("");
    folders.closeSummary();
    counting.touch("notes/ideas.md");
    await until(() => folders.isStale(""), "the project is stale");
    await count("");
    expect(folders.isStale("")).toBe(false);
    // a change outside the folder leaves another folder alone
    await count("docs");
    counting.touch("notes/todo.txt");
    await until(() => folders.isStale(""), "the project is stale again");
    expect(folders.isStale("docs")).toBe(false);
  });
});

describe("the totals counted by themselves", () => {
  it("counts the folders that open, one after the other, quietly", async () => {
    await tokens.setFolderTotals(true);
    folders.folderOpened(new Set(["docs", "notes", "src"]));
    await until(() => folders.counted.length === 3, `three totals: ${folders.counted.length}`);
    expect(counting.calls.filter((c) => c.endsWith(" auto"))).toHaveLength(3);
    expect(folders.summary, "no dialog for a total nobody asked for").toBeNull();
    expect(windowStore.notices.some((n) => n.kind === "note")).toBe(false);
    // a folder that is open already is not counted again; a stale one is
    counting.calls.length = 0;
    folders.folderOpened(new Set(["docs", "notes", "src"]));
    expect(counting.calls).toEqual([]);
    counting.touch("docs/guide.md");
    await until(() => folders.isStale("docs"), "stale");
    folders.folderOpened(new Set(["notes", "src"]));
    folders.folderOpened(new Set(["notes", "src", "docs"]));
    await until(() => !folders.isStale("docs"), "docs counted again");
  });

  it("leaves a folder that is not small alone", async () => {
    counting.thresholds.autoFiles = 1;
    await tokens.setFolderTotals(true);
    folders.folderOpened(new Set(["docs", "notes"]));
    await until(
      () => counting.calls.filter((c) => c.endsWith(" auto")).length === 2,
      "both were asked",
    );
    await until(() => folders.job.state === "idle", "idle");
    // docs holds two files (the guide and a picture): over the limit; notes holds two as well
    expect(folders.counted).toEqual([]);
    expect(windowStore.notices.some((n) => n.kind === "note")).toBe(false);
  });

  it("does nothing while the switch is off, and forgets the queue when it goes off", async () => {
    folders.folderOpened(new Set(["docs", "notes"]));
    expect(counting.calls).toEqual([]);
    counting.stepMs = 30;
    await tokens.setFolderTotals(true);
    folders.folderOpened(new Set(["docs", "notes", "src"]));
    folders.stopAuto();
    // the one that had started finishes, the ones that waited are forgotten
    await until(
      () => folders.counted.length === 1 && folders.job.state === "idle",
      "the first ends",
    );
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(counting.calls.filter((c) => c.endsWith(" auto"))).toHaveLength(1);
    expect(folders.counted).toHaveLength(1);
  });
});

describe("what the plugin adds", () => {
  it("adds the badge, the menu entries and the commands, and leaves nothing when off", async () => {
    stopFolders?.();
    stopFolders = null;
    stopTokens?.();
    stopTokens = null;
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    await host.enable("token-count");
    const own = registry.ownedBy("token-count");
    const kinds = [...new Set(own.map((c) => c.kind))].sort();
    expect(kinds).toEqual([
      "command",
      "outlineColumn",
      "projectSection",
      "settingsSection",
      "statusItem",
      "treeBadge",
      "treeMenu",
    ]);
    const ids = own.flatMap((c) => ("id" in c ? [c.id] : []));
    expect(ids).toContain("tokens.countFolder");
    expect(ids).toContain("tokens.countProject");
    const menus = own.filter((c) => c.kind === "treeMenu");
    const count = menus.find((m) => m.id === "token-count.folder-count");
    const summary = menus.find((m) => m.id === "token-count.folder-summary");
    // the menu is for folders, the summary for folders that were counted
    expect(count?.kind === "treeMenu" && count.applies("docs", true)).toBe(true);
    expect(count?.kind === "treeMenu" && count.applies("README.md", false)).toBe(false);
    expect(summary?.kind === "treeMenu" && summary.applies("docs", true)).toBe(false);
    await until(() => tokens.state !== null, "the stores started with the plugin");
    await choose("chars4");
    await folders.load(null);
    await folders.load(MOCK_FOLDER);
    await count_run(folders, "docs");
    expect(summary?.kind === "treeMenu" && summary.applies("docs", true)).toBe(true);
    expect(host.disable("token-count"), "contributions left after deactivate").toBe(0);
    expect(registry.items).toHaveLength(0);
    // the folder count goes with the plugin: no badge, no summary, no job
    expect(folders.badge("docs", true)).toBeNull();
    expect(folders.summary).toBeNull();
    expect(folders.job.state).toBe("idle");
    expect(folders.counted).toEqual([]);
  });
});

async function count_run(store: typeof folders, folder: string): Promise<void> {
  await store.count(folder);
  await until(() => store.summary !== null, `the summary of ${folder}`);
}
