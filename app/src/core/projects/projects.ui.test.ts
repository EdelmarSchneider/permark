// @vitest-environment jsdom
// Projects inside the folder on screen (`projects.md` §12, design §5): the Explorer strip, the project node with its
// menu and card, Settings > Projects (groups, rules, consent mode, read-only), the Changes rows and the search scope,
// mounted against the mock backend's fixtures.
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER } from "../../api/mock";
import {
  MOCK_PROJECTS_BROKEN,
  MOCK_PROJECTS_FRESH,
  MOCK_PROJECTS_LISTED,
  MOCK_PROJECTS_MANY,
  MOCK_PROJECTS_ONE,
} from "../../api/mockProjects";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});
Element.prototype.scrollIntoView = () => {};
// jsdom has no layout: the list is 300 x 400, one row is 26
Element.prototype.getBoundingClientRect = function () {
  return {
    left: 0,
    top: 0,
    right: 300,
    bottom: 400,
    width: 300,
    height: 400,
    x: 0,
    y: 0,
  } as DOMRect;
};
if (typeof CSS === "undefined" || typeof CSS.escape !== "function") {
  (globalThis as { CSS?: unknown }).CSS = {
    escape: (s: string) => s.replace(/["\\]/g, "\\$&"),
  };
}

const { api } = await import("../../api");
const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const { default: ExplorerPanel } = await import("../explorer/ExplorerPanel.svelte");
const { ExplorerStore } = await import("../explorer/store.svelte");
const { default: ProjectsSection } = await import("./ProjectsSection.svelte");
const { default: SearchPanel } = await import("../search/SearchPanel.svelte");
const { SearchPanelStore } = await import("../search/panel.svelte");
const { default: FeedPanel } = await import("../../plugins/live-feed/FeedPanel.svelte");
const { feed } = await import("../../plugins/live-feed/store.svelte");
const { entry } = await import("../../plugins/live-feed/testing");
const { ContributionRegistry } = await import("../plugins/registry.svelte");
const { conditions, neutralContext } = await import("../conditions/context.svelte");
const { documents } = await import("../doc/document.svelte");
const { ui } = await import("../shell/ui.svelte");
const { projects } = await import("./instance.svelte");
const { default: Notices } = await import("../shell/Notices.svelte");
const { windowStore } = await import("../window.svelte");

let target: HTMLElement;
let cleanups: (() => void)[] = [];

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

function press(el: Element, key: string, init: KeyboardEventInit = {}): void {
  el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }));
  flushSync();
}

/** Opens a folder in the mock and follows its projects. */
async function openFolder(root: string): Promise<void> {
  await mock.openPath(root, "thisTab");
  conditions.set({ ...neutralContext(), workspace: "project", plugins: [] });
  const stop = await projects.start(root);
  cleanups.push(stop);
}

/** The Explorer of the open folder, mounted. */
async function explorer(): Promise<{ store: InstanceType<typeof ExplorerStore> }> {
  const store = new ExplorerStore(api, {
    fail: () => {},
    note: () => {},
    view: () => "main",
    linkUpdate: () => "never",
    openProject: () => {},
  });
  const stop = await store.start();
  const app = mount(ExplorerPanel, {
    target,
    props: { store, registry: new ContributionRegistry() },
  });
  flushSync();
  await tick();
  cleanups.push(() => {
    void unmount(app);
    stop();
  });
  return { store };
}

function section(): void {
  const app = mount(ProjectsSection, { target });
  flushSync();
  cleanups.push(() => void unmount(app));
}

const strip = (): HTMLElement | null =>
  target.querySelector<HTMLElement>('[role="region"][aria-label="Projects in this folder"]');
const button = (text: string, scope: ParentNode = target): HTMLButtonElement | undefined =>
  Array.from(scope.querySelectorAll<HTMLButtonElement>("button")).find(
    (b) => b.textContent?.trim() === text,
  );
const projectRows = (): HTMLElement[] =>
  Array.from(target.querySelectorAll<HTMLElement>('[role="treeitem"][data-project]'));

beforeEach(() => {
  mock.probe.projects.reset();
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  for (const clean of cleanups.reverse()) clean();
  cleanups = [];
  ui.closeSettingsPage();
  documents.setWorkspace(null);
});

describe("Explorer: the strip and the project node", () => {
  it("a plain folder shows nothing new: no strip, no project node, no card (A0)", async () => {
    await openFolder(MOCK_FOLDER);
    await explorer();
    expect(strip()).toBeNull();
    expect(projectRows()).toEqual([]);
    expect(target.querySelector('[role="dialog"]')).toBeNull();
    expect(
      mock.probe.projects.calls.filter((c) => c !== "projects_view" && c.includes(MOCK_FOLDER)),
    ).toEqual([]);
  });

  it("asks once under the list; Include all answers, the nodes change, focus back on the list (A1, A2)", async () => {
    await openFolder(MOCK_PROJECTS_FRESH);
    await explorer();
    const region = strip();
    expect(region?.textContent).toContain("This folder holds 4 projects.");
    expect(region?.textContent).toContain("hub-b includes 1 project of its own.");
    expect(
      ["Include all", "Choose…", "No thanks"].map((t) => button(t, region!) !== undefined),
    ).toEqual([true, true, true]);
    // the strip comes after the list (Tab from the list reaches it) and never inside it
    const list = target.querySelector('[role="tree"]')!;
    expect(list.compareDocumentPosition(region!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(list.contains(region)).toBe(false);
    button("Include all", region!)!.click();
    await until(() => strip() === null, "strip answered");
    await until(() => projectRows().length > 0, "project nodes");
    const site = target.querySelector<HTMLElement>('[data-path="site"]');
    expect(site?.dataset["project"]).toBe("");
    expect(site?.getAttribute("aria-label")).toBe("site, project");
    expect(document.activeElement).toBe(list);
  });

  it("the code offer waits while (and after) the projects strip asks", async () => {
    await openFolder(MOCK_PROJECTS_FRESH);
    await explorer();
    expect(strip()).not.toBeNull();
    expect(target.querySelector('[aria-label="Code files"]')).toBeNull();
    await projects.declineOffered();
    flushSync();
    expect(strip()).toBeNull();
    expect(target.querySelector('[aria-label="Code files"]')).toBeNull();
  });

  it("a folder with a list: new project strip with Include | No thanks (no Choose for one)", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    await explorer();
    const region = strip();
    // F8: where it is
    expect(region?.textContent).toContain("New project: tools/new-app.");
    expect(button("Include", region!)).toBeDefined();
    expect(button("Choose…", region!)).toBeUndefined();
    // included projects wear their card name and an icon; not included ones stay plain folders
    const hub = target.querySelector<HTMLElement>('[data-path="hubs"]');
    expect(hub?.dataset["project"]).toBeUndefined();
  });

  it("the menu by keyboard (Shift+F10) offers the card; the card takes focus, Esc gives it back (A6)", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    const { store } = await explorer();
    await store.toggle("tools");
    flushSync();
    await until(() => target.querySelector('[data-path="tools/notes-app"]') !== null, "row");
    const list = target.querySelector<HTMLElement>('[role="tree"]')!;
    list.focus();
    store.select("tools/notes-app", "single");
    store.focus = "tools/notes-app";
    flushSync();
    press(list, "F10", { shiftKey: true });
    const menu = document.querySelector('[role="menu"]');
    expect(menu?.textContent).toContain("Project details");
    expect(menu?.textContent).toContain("Project settings…");
    // F17: the card's words for the same action
    expect(menu?.textContent).toContain("Open in new tab");
    expect(menu?.textContent).not.toContain("Open as project");
    const details = Array.from(menu!.querySelectorAll<HTMLElement>('[role="menuitem"]')).find((m) =>
      m.textContent?.includes("Project details"),
    );
    details!.click();
    flushSync();
    await until(() => document.querySelector('[role="dialog"]') !== null, "card loaded");
    const card = document.querySelector<HTMLElement>('[role="dialog"]');
    expect(button("Project settings…", card!)).toBeDefined();
    expect(card?.getAttribute("aria-label")).toBe("notes-app details");
    expect(card?.textContent).toContain("tools/notes-app");
    await until(() => document.activeElement === button("Open in new tab", card!), "focus in card");
    press(document.activeElement!, "Escape");
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(list);
    expect(store.focus).toBe("tools/notes-app");
  });

  it("hovering a project node 500 ms shows the card; it stays while the pointer is inside, then goes", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    const { store } = await explorer();
    await store.toggle("tools");
    flushSync();
    await until(() => target.querySelector('[data-path="tools/notes-app"]') !== null, "row");
    // the card's code is loaded once (as after a first card)
    await import("./ProjectCard.svelte");
    vi.useFakeTimers();
    try {
      const row = target.querySelector<HTMLElement>('[data-path="tools/notes-app"]')!;
      row.dispatchEvent(new Event("pointerenter"));
      vi.advanceTimersByTime(499);
      flushSync();
      expect(document.querySelector('[role="dialog"]')).toBeNull();
      vi.advanceTimersByTime(1);
      flushSync();
      await vi.dynamicImportSettled();
      flushSync();
      const card = document.querySelector<HTMLElement>('[role="dialog"]')!;
      expect(card).not.toBeNull();
      // the pointer moves from the row into the card: it stays
      row.dispatchEvent(new Event("pointerleave"));
      card.dispatchEvent(new Event("pointerenter"));
      vi.advanceTimersByTime(1000);
      flushSync();
      expect(document.querySelector('[role="dialog"]')).not.toBeNull();
      card.dispatchEvent(new Event("pointerleave"));
      vi.advanceTimersByTime(300);
      flushSync();
      expect(document.querySelector('[role="dialog"]')).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("an unreadable list: the Explorer says so; Show the problem opens Settings on it, focused (F7)", async () => {
    await openFolder(MOCK_PROJECTS_BROKEN);
    await explorer();
    const region = strip();
    expect(region?.textContent).toContain("This folder's project list can't be read.");
    expect(["Include all", "No thanks"].map((t) => button(t, region!))).toEqual([
      undefined,
      undefined,
    ]);
    button("Show the problem", region!)!.click();
    flushSync();
    expect(ui.settingsPage).toEqual({ status: "open", section: "projects" });
    section();
    const alert = target.querySelector<HTMLElement>('[role="alert"]');
    await until(() => document.activeElement === alert, "focus on the problem");
    expect(projects.ask).toBeNull();
  });
});

describe("Settings > Projects", () => {
  it("groups the projects, filters them, opens the rules of one (keyboard: Rules)", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    section();
    const text = target.textContent ?? "";
    expect(text).toContain("Included (3)");
    expect(text).toContain("Not included (2)");
    expect(text).toContain("Not found (1)");
    expect(text).toContain("was at hubs/hub-x");
    expect(text).toContain("The list is saved in this folder (.permark/projects.toml)");
    const filter = target.querySelector<HTMLInputElement>('input[type="search"]')!;
    filter.value = "notes";
    filter.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(target.querySelectorAll("[data-project]")).toHaveLength(1);
    const rules = button("Details and rules")!;
    expect(rules.getAttribute("aria-expanded")).toBe("false");
    rules.click();
    flushSync();
    expect(rules.getAttribute("aria-expanded")).toBe("true");
    expect(target.textContent).toContain("Hidden files");
    // F16: the folder name reads as a name
    expect(target.textContent).toContain(
      "Files that “workspace-listed” keeps private stay hidden either way.",
    );
    expect(target.textContent).toContain("Saved in your profile");
    // the agent connection of the folder is off: the choice says so, and still works
    expect(target.textContent).toContain("The agent connection of workspace-listed is off.");
    const same = Array.from(target.querySelectorAll<HTMLElement>('[role="radio"]')).find(
      (r) => r.textContent?.trim() === "Same as workspace-listed",
    )!;
    same.click();
    await until(() => projects.memberAt("tools/notes-app")?.agent === "same", "agent saved");
    // "Leave out shown" acts on what the filter shows only
    button("Leave out shown")!.click();
    await until(() => projects.memberAt("tools/notes-app") === null, "left out");
    expect(projects.memberAt("hubs/hub-b")).not.toBeNull();
  });

  it("a card field saves on Enter", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    projects.openSettings(projects.memberAt("hubs/hub-b"));
    section();
    await until(() => target.querySelector('input[id$="-name"]') !== null, "row opened by ask");
    const name = target.querySelector<HTMLInputElement>('input[id$="-name"]')!;
    expect(name.value).toBe("PerMark software");
    name.value = "Hub B (software)";
    press(name, "Enter");
    await until(() => projects.memberAt("hubs/hub-b")?.name === "Hub B (software)", "card saved");
  });

  it("consent mode: heading, all checked, Include checked saves the checked ones (A4)", async () => {
    await openFolder(MOCK_PROJECTS_FRESH);
    projects.beginChoose();
    section();
    const heading = target.querySelector<HTMLElement>("[data-autofocus]");
    expect(heading?.textContent).toBe("Choose projects to include");
    const boxes = Array.from(target.querySelectorAll<HTMLInputElement>('input[type="checkbox"]'));
    expect(boxes).toHaveLength(4);
    expect(boxes.every((b) => b.checked)).toBe(true);
    expect(boxes[0]?.getAttribute("aria-label")).toBe("Include hub-b");
    const site = boxes.find((b) => b.getAttribute("aria-label") === "Include site")!;
    site.click();
    flushSync();
    expect(projects.choosing?.has("site")).toBe(false);
    // nothing saved yet
    expect(mock.probe.projects.calls.some((c) => c.startsWith("projects_include"))).toBe(false);
    button("Include checked")!.click();
    await until(() => projects.members.size === 3, "3 included");
    expect(projects.choosing).toBeNull();
    expect(projects.view?.rows.find((r) => r.path === "site")?.state).toBe("declined");
  });

  it("an unreadable list: error card with Show the file; checkboxes off with a reason", async () => {
    await openFolder(MOCK_PROJECTS_BROKEN);
    section();
    const alert = target.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain(
      "The project list in this folder can't be read (line 3). PerMark won't change it until it is fixed.",
    );
    expect(button("Show the file", alert!)).toBeDefined();
    const box = target.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    expect(box.disabled).toBe(true);
    expect(box.title).toBe("The project list can't be changed until it can be read.");
  });

  it("40 projects with long and Unicode names: every row named, long ones cut with the full text kept", async () => {
    await openFolder(MOCK_PROJECTS_MANY);
    section();
    const rows = target.querySelectorAll("[data-project]");
    expect(rows).toHaveLength(40);
    expect(target.textContent).toContain("Included (36)");
    const long = Array.from(target.querySelectorAll<HTMLElement>(".name")).find((n) =>
      n.textContent?.startsWith("a-very-long"),
    );
    expect(long?.title.length).toBeGreaterThan(80);
    expect(target.textContent).toContain("数据分析项目");
    const filter = target.querySelector<HTMLInputElement>('input[type="search"]')!;
    filter.value = "zzz-nothing";
    filter.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(target.textContent).toContain("No project matches.");
  });
});

describe("Changes panel and search", () => {
  it("project news are rows; paths inside a project say which one", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    feed.items = [];
    feed.take([
      entry(1, "hubs/hub-b/docs/plan.md"),
      entry(2, "README.md"),
      {
        item: "project",
        id: 3,
        atMs: 5000,
        event: { kind: "gone", name: "hub-x", path: "hubs/hub-x" },
      },
      {
        item: "project",
        id: 4,
        atMs: 6000,
        event: { kind: "moved", name: "notes-app", path: "tools/notes-app" },
      },
    ]);
    const app = mount(FeedPanel, { target });
    flushSync();
    cleanups.push(() => {
      void unmount(app);
      feed.items = [];
    });
    const text = target.textContent ?? "";
    expect(text).toContain("hub-b · docs");
    expect(text).toContain("Project hub-x is no longer here");
    expect(text).toContain("Project notes-app moved to tools/notes-app");
    const row = (text: string): HTMLButtonElement | undefined =>
      Array.from(target.querySelectorAll<HTMLButtonElement>("button.row")).find(
        (b) => b.querySelector(".name")?.textContent === text,
      );
    expect(row("Project notes-app moved to tools/notes-app")?.disabled).toBe(false);
    // F18: the sentence wraps instead of losing its end
    expect(
      row("Project notes-app moved to tools/notes-app")?.querySelector(".name.wraps"),
    ).not.toBeNull();
    // F6: a project not found now leads to its row in Settings (Locate… | Remove from list)
    const gone = row("Project hub-x is no longer here")!;
    expect(gone.disabled).toBe(false);
    gone.click();
    flushSync();
    expect(ui.settingsPage).toEqual({ status: "open", section: "projects" });
    expect(projects.ask).toMatchObject({ kind: "row", key: "hubs/hub-x" });
    section();
    await until(
      () => (document.activeElement as HTMLElement | null)?.textContent?.trim() === "Locate…",
      "focus on Locate…",
    );
  });

  it("a long destination is cut in its middle, the full one in the tooltip (F18)", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    feed.items = [];
    const far = "deep/folders/that/go/on/and/on/for/a/while/notes-app";
    feed.take([
      {
        item: "project",
        id: 5,
        atMs: 5000,
        event: { kind: "moved", name: "notes-app", path: far },
      },
    ]);
    const app = mount(FeedPanel, { target });
    flushSync();
    cleanups.push(() => {
      void unmount(app);
      feed.items = [];
    });
    const shown = target.querySelector<HTMLButtonElement>("button.row")!;
    expect(shown.title).toBe(`Project notes-app moved to ${far}`);
    expect(shown.querySelector(".name")?.textContent).toContain("…/notes-app");
  });

  it("search scope: only with projects; This project needs a document inside one", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    const spy = vi.spyOn(api, "searchSections");
    const store = new SearchPanelStore(api, () => projects.ownerOf(documents.path)?.uid ?? null);
    const app = mount(SearchPanel, { target, props: { root: MOCK_PROJECTS_LISTED, store } });
    flushSync();
    cleanups.push(() => {
      void unmount(app);
      spy.mockRestore();
    });
    const radios = (): HTMLElement[] =>
      Array.from(target.querySelectorAll<HTMLElement>('[role="radiogroup"] [role="radio"]'));
    // F14: what "all" covers
    expect(radios().map((r) => r.textContent?.trim())).toEqual([
      "Whole folder",
      "This project (…)",
    ]);
    // F12: the reason fits the moment (no document open), also for a screen reader
    const off = radios()[1]!;
    expect(off.getAttribute("aria-disabled")).toBe("true");
    const why = "Open a document inside a project to search only that project.";
    expect(off.title).toBe(why);
    expect(document.getElementById(off.getAttribute("aria-describedby") ?? "")?.textContent).toBe(
      why,
    );
    radios()[1]!.click();
    flushSync();
    expect(store.scope).toBe("all");
    store.scope = "this";
    store.setQuery("plan");
    await until(() => spy.mock.calls.length > 0, "searched");
    expect(spy.mock.calls.at(-1)?.[2].project).toBeUndefined();
  });

  it("search scope: This project sends the project of the open document", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    const spy = vi.spyOn(api, "searchSections");
    const store = new SearchPanelStore(
      api,
      () => projects.ownerOf("hubs/hub-b/README.md")?.uid ?? null,
    );
    store.setScope("this");
    store.setQuery("hub");
    await until(() => spy.mock.calls.length > 0, "searched");
    expect(spy.mock.calls.at(-1)?.[2].project).toBe("k7m2q7xd2a");
    spy.mockRestore();
    store.dispose();
  });
});

describe("UX review round 2: focus, words, places", () => {
  /** The window's notices, mounted (where Undo lives). */
  function notices(): void {
    windowStore.notices = [];
    const app = mount(Notices, { target, props: { store: windowStore } });
    flushSync();
    cleanups.push(() => {
      void unmount(app);
      for (const n of windowStore.notices) windowStore.dismiss(n.id);
    });
  }

  const noticeButton = (text: string): HTMLButtonElement | undefined =>
    button(text, target.querySelector(".notices") ?? target);

  it("a project node shows its folder name; the short name follows, muted, only when it differs (F1)", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    const { store } = await explorer();
    await store.toggle("hubs");
    flushSync();
    await until(() => target.querySelector('[data-path="hubs/hub-b"]') !== null, "row");
    const hub = target.querySelector<HTMLElement>('[data-path="hubs/hub-b"]');
    // card name "PerMark software", short "hub-b" = folder name: the folder name alone
    expect(hub?.querySelector(".name")?.textContent).toBe("hub-b");
    expect(hub?.getAttribute("aria-label")).toBe("hub-b, project");
    await projects.setCard(
      { kind: "member", uid: "k7m2q7xd2a" },
      { field: "short", value: "PerMark" },
    );
    flushSync();
    await until(() => hub?.querySelector(".alias") !== null, "alias shown");
    expect(hub?.querySelector(".folder")?.textContent).toBe("hub-b");
    expect(hub?.querySelector(".alias")?.textContent).toBe("PerMark");
    expect(hub?.getAttribute("aria-label")).toBe("hub-b, project PerMark");
  });

  it("one project: singular words, Include | No thanks, no Choose (F15)", async () => {
    await openFolder(MOCK_PROJECTS_ONE);
    await explorer();
    const region = strip()!;
    expect(region.textContent).toContain(
      "This folder holds 1 project: template-app. Include it to follow its changes and unread documents from here.",
    );
    expect(["Include", "Choose…", "No thanks"].map((t) => button(t, region) !== undefined)).toEqual(
      [true, false, true],
    );
  });

  it("No thanks says where to change the mind (F9)", async () => {
    await openFolder(MOCK_PROJECTS_FRESH);
    await explorer();
    notices();
    button("No thanks", strip()!)!.click();
    await until(() => strip() === null, "answered");
    await until(
      () =>
        target.querySelector(".notices")?.textContent?.includes("Settings > Projects inside") ===
        true,
      "notice",
    );
    expect(target.querySelector(".notices")?.textContent).toContain(
      "You can include projects later in Settings > Projects inside.",
    );
  });

  it("Undo of Include all: the question is back, focus on it, the reader hears it (F3)", async () => {
    await openFolder(MOCK_PROJECTS_FRESH);
    await explorer();
    notices();
    button("Include all", strip()!)!.click();
    await until(() => noticeButton("Undo") !== undefined, "notice with Undo");
    const undo = noticeButton("Undo")!;
    undo.focus();
    undo.click();
    flushSync();
    await until(() => strip() !== null, "asked again");
    await until(
      () => document.activeElement === button("Include all", strip()!),
      "focus on the strip",
    );
    await until(() => projects.announcement !== "", "announced");
    expect(projects.announcement).toBe("Undone: 4 projects are no longer included.");
  });

  it("leaving a project out: focus to the next row of its group, the move is said (F4)", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    section();
    const box = (name: string): HTMLInputElement | null =>
      target.querySelector<HTMLInputElement>(`input[aria-label="Include ${name}"]`);
    box("notes-app")!.focus();
    box("notes-app")!.click();
    await until(() => projects.memberAt("tools/notes-app") === null, "left out");
    // notes-app was the last included row: the one before it takes the focus
    await until(() => document.activeElement === box("spec"), "focus on the next row");
    await until(() => projects.announcement !== "", "announced");
    expect(projects.announcement).toBe("notes-app moved to Not included.");
    expect(target.textContent).toContain("Included (2)");
  });

  it("Remove from list: focus stays in the page, Undo brings the entry back as it was (F5)", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    ui.openSettingsPage("projects");
    section();
    notices();
    const before = projects.view!.rows.find((r) => r.state === "missing")!;
    const remove = button("Remove from list")!;
    remove.focus();
    remove.click();
    flushSync();
    await until(() => !(target.textContent ?? "").includes("was at hubs/hub-x"), "row gone");
    // no other missing row: focus on the filter
    await until(
      () => document.activeElement === target.querySelector('input[type="search"]'),
      "focus on the filter",
    );
    expect(target.querySelector(".notices")?.textContent).toContain("hub-x removed from the list.");
    noticeButton("Undo")!.click();
    flushSync();
    await until(() => (target.textContent ?? "").includes("was at hubs/hub-x"), "row back");
    expect(projects.view!.rows.find((r) => r.uid === before.uid)).toEqual(before);
    await until(
      () => (document.activeElement as HTMLElement | null)?.textContent?.trim() === "Locate…",
      "focus on the row again",
    );
    await until(() => projects.announcement !== "", "announced");
    expect(projects.announcement).toBe("hub-x is back in the list.");
    expect(mock.probe.projects.calls.some((c) => c.startsWith("projects_remove"))).toBe(false);
  });

  it("the card's Project settings… opens that project's row, focused on its first rule; no stale read (F11, F19)", async () => {
    const warn = vi.spyOn(console, "warn");
    await openFolder(MOCK_PROJECTS_LISTED);
    const { store } = await explorer();
    await store.toggle("tools");
    flushSync();
    await until(() => target.querySelector('[data-path="tools/notes-app"]') !== null, "row");
    const list = target.querySelector<HTMLElement>('[role="tree"]')!;
    list.focus();
    store.select("tools/notes-app", "single");
    store.focus = "tools/notes-app";
    flushSync();
    press(list, "F10", { shiftKey: true });
    Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]'))
      .find((m) => m.textContent?.includes("Project details"))!
      .click();
    flushSync();
    await until(() => document.querySelector('[role="dialog"]') !== null, "card");
    button("Project settings…", document.querySelector('[role="dialog"]')!)!.click();
    flushSync();
    expect(ui.settingsPage).toEqual({ status: "open", section: "projects" });
    section();
    const row = target.querySelector<HTMLElement>('[data-project="tools/notes-app"]')!;
    await until(() => row.querySelector(".rules-body") !== null, "row opened");
    await until(
      () =>
        document.activeElement?.closest('[data-project="tools/notes-app"] .rules-body') !== null,
      "focus on its first rule",
    );
    expect(warn.mock.calls.some((c) => String(c[0]).includes("derived_inert"))).toBe(false);
    warn.mockRestore();
  });
});
