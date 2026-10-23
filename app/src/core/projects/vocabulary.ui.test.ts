// @vitest-environment jsdom
// Official | local | unlisted on screen (`projects.md` §7, §12; round 4): marks on tag chips (Tags tab, tag editor,
// strip), "Make official in {root}" (asks, says where, writes), the category field of a card (official names offered,
// its mark, promote), the official lists in Settings > Projects inside, the Tags row, the card's counts. Mounted
// against the mock backend (`workspace-vocab`: official `needs-review`, `spec` (blue), category `software`; hub-b lists
// `draft` + `spec` of its own, site lists category `website`).
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER } from "../../api/mock";
import { MOCK_PROJECTS_LISTED, MOCK_PROJECTS_VOCAB } from "../../api/mockProjects";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});
Element.prototype.scrollIntoView = () => {};

const { api } = await import("../../api");
const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const { default: TagsPanel } = await import("../../plugins/tags/TagsPanel.svelte");
const { default: TagEditor } = await import("../../plugins/tags/TagEditor.svelte");
const { default: TagStrip } = await import("../../plugins/tags/TagStrip.svelte");
const { tags } = await import("../../plugins/tags/store.svelte");
const { comments } = await import("../../plugins/comments/store.svelte");
const { default: ProjectsSection } = await import("./ProjectsSection.svelte");
const { default: ProjectCard } = await import("./ProjectCard.svelte");
const { projects } = await import("./instance.svelte");
const { contributions } = await import("../plugins/instance.svelte");
const { conditions, neutralContext } = await import("../conditions/context.svelte");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { documents } = await import("../doc/document.svelte");
const { ui } = await import("../shell/ui.svelte");
const { windowStore } = await import("../window.svelte");

let target: HTMLElement;
let cleanups: (() => void)[] = [];
let started = false;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

function press(el: Element, key: string): void {
  el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  flushSync();
}

function type(input: HTMLInputElement, text: string): void {
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

/** Opens a folder of the mock with the tags plugin's store following it (and its projects). */
async function openFolder(root: string): Promise<void> {
  await mock.openPath(root, "thisTab");
  if (!started) {
    await windowStore.start();
    started = true;
  }
  conditions.set({ ...neutralContext(), workspace: "project", plugins: ["tags"] });
  cleanups.push(await projects.start(root));
  const stop = tags.start();
  cleanups.push(stop);
  await until(() => tags.index !== null && tags.overview !== null, "tags loaded");
}

function show<P extends Record<string, unknown>>(
  component: import("svelte").Component<P>,
  props: P = {} as P,
): void {
  const app = mount(component, { target, props });
  flushSync();
  cleanups.push(() => void unmount(app));
}

const button = (text: string, scope: ParentNode = target): HTMLButtonElement | undefined =>
  Array.from(scope.querySelectorAll<HTMLButtonElement>("button")).find(
    (b) => b.textContent?.trim() === text,
  );
const chip = (name: string, scope: ParentNode = target): HTMLElement | null =>
  scope.querySelector<HTMLElement>(`.chip[data-tag="${name}"]`);
const statusOf = (name: string): string | null =>
  chip(name)?.querySelector("[data-status]")?.getAttribute("data-status") ?? null;
const notes = (): string => JSON.stringify(windowStore.notices);

beforeEach(() => {
  mock.probe.projects.reset();
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  for (const clean of cleanups.reverse()) clean();
  cleanups = [];
  mock.probe.plugins.organizing.reset();
  // files a test added to the in-memory project go (the next test sees the fixture only)
  for (const path of [...mock.probe.project.files.keys()]) {
    if (path.startsWith("many/") || path === "notes.md") mock.probe.project.files.delete(path);
  }
  ui.closeSettingsPage();
  documents.setWorkspace(null);
  vi.restoreAllMocks();
});

describe("tag chips say official | local | unlisted", () => {
  it("Tags tab: a shape per status, its words in the tooltip and the chip's name; unlisted is dashed", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    show(TagsPanel);
    await until(() => chip("spec") !== null && projects.view !== null, "chips");
    await until(() => statusOf("draft") === "local", "marks");
    expect(statusOf("spec")).toBe("official");
    expect(statusOf("roadmap")).toBe("unlisted");
    expect(statusOf("lab")).toBe("unlisted");
    // the words: in the button (screen reader) and the mark's tooltip; the owner by its short name
    const face = chip("draft")!.querySelector("button.face")!;
    expect(face.textContent).toContain("Local to hub-b");
    expect(chip("draft")!.querySelector("[data-status]")!.getAttribute("title")).toBe(
      "Local to hub-b",
    );
    expect(chip("roadmap")!.classList.contains("unlisted")).toBe(true);
    expect(chip("spec")!.classList.contains("unlisted")).toBe(false);
  });

  it("a tag both local (hub-b's own list) and official is official, with the official look", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    show(TagsPanel);
    await until(() => statusOf("spec") === "official", "official wins");
    expect(chip("spec")!.classList.contains("colored")).toBe(true);
  });

  it("a plain folder draws no mark and offers no promote (A0)", async () => {
    await openFolder(MOCK_FOLDER);
    show(TagsPanel);
    await until(() => target.querySelectorAll(".list .chip").length > 0, "chips");
    expect(target.querySelector("[data-status]")).toBeNull();
    target.querySelector<HTMLButtonElement>(".list li button.icon")!.click();
    flushSync();
    expect(target.querySelector('[role="dialog"]')).not.toBeNull();
    expect(target.textContent).not.toContain("Make official");
    expect(mock.probe.projects.calls.some((c) => c.startsWith("vocabulary_promote"))).toBe(false);
  });

  it("the tag editor and the strip show the marks of the file's project", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    show(TagEditor, { path: "hub-b/plan.md" });
    await until(() => statusOf("draft") === "local" && statusOf("spec") === "official", "editor");
    cleanups.pop()?.();
    // the strip under the header of the shown document: the marks of its own project chain
    vi.spyOn(documents, "shown", "get").mockReturnValue("hub-b/plan.md");
    show(TagStrip);
    await until(() => statusOf("draft") === "local", "strip marks");
    expect(statusOf("spec")).toBe("official");
  });

  it("the strip of a plain folder asks nothing more than before (marks dormant)", async () => {
    await openFolder(MOCK_FOLDER);
    const asked = vi.spyOn(tags, "fileMeta");
    const path = tags.index!.entries[0]!.path;
    vi.spyOn(documents, "shown", "get").mockReturnValue(path);
    show(TagStrip);
    await until(() => target.querySelector(".strip") !== null, "strip");
    await tick();
    expect(asked).not.toHaveBeenCalled();
    expect(target.querySelector("[data-status]")).toBeNull();
  });

  it("200 tags with long and Unicode names: every chip drawn with its mark and full name", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    const names: string[] = [];
    for (let n = 0; n < 200; n += 1) {
      const name =
        n % 3 === 0
          ? `long-${"x".repeat(30)}-${n}`
          : n % 3 === 1
            ? `标签-ünïcödé-${n}`
            : `tag-${n}`;
      names.push(name);
      mock.probe.project.add(`many/doc-${n}.md`, `---\ntags: [${name}]\n---\n# ${n}\n`);
    }
    await tags.load();
    show(TagsPanel);
    await until(() => target.querySelectorAll(".list .chip").length >= 200, "200 chips");
    for (const name of [names[0]!, names[1]!, names[199]!]) {
      // the visible name (the screen reader's rest follows it in the same box)
      expect(chip(name)?.querySelector(".name")?.firstChild?.textContent).toBe(name);
      expect(statusOf(name)).toBe("unlisted");
    }
  });
});

describe("Make official in {root}", () => {
  async function popoverFor(name: string): Promise<HTMLElement> {
    show(TagsPanel);
    await until(() => statusOf(name) !== null, "marks");
    chip(name)!.parentElement!.querySelector<HTMLButtonElement>("button.icon")!.click();
    flushSync();
    return target.querySelector<HTMLElement>('[role="dialog"]')!;
  }

  it("asks first, says which file it writes, then the mark turns official and it is said", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    const pop = await popoverFor("roadmap");
    const opener = button("Make official in workspace-vocab", pop)!;
    expect(opener).toBeDefined();
    opener.click();
    flushSync();
    await tick();
    expect(mock.probe.projects.calls.some((c) => c.startsWith("vocabulary_promote"))).toBe(false);
    const ask = pop.querySelector<HTMLElement>('[role="group"]')!;
    expect(ask.textContent).toContain(
      "“roadmap” becomes an official tag for every project in workspace-vocab.",
    );
    expect(ask.textContent).toContain(
      "PerMark saves the official list in this folder (.permark/vocabulary.toml).",
    );
    await until(() => document.activeElement?.textContent?.trim() === "Make official", "focus");
    button("Make official", pop)!.click();
    await until(() => statusOf("roadmap") === "official", "official now");
    expect(mock.probe.projects.calls).toContain("vocabulary_promote tag roadmap");
    await until(() => projects.announcement.includes("is now an official tag"), "said");
    expect(projects.announcement).toBe("“roadmap” is now an official tag of workspace-vocab.");
  });

  it("Esc | Cancel goes back to the button, nothing written", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    const pop = await popoverFor("lab");
    button("Make official in workspace-vocab", pop)!.click();
    flushSync();
    await until(() => document.activeElement?.textContent?.trim() === "Make official", "asked");
    press(document.activeElement!, "Escape");
    await until(
      () => document.activeElement?.textContent?.trim() === "Make official in workspace-vocab",
      "back on the button",
    );
    // Esc in the question did not close the popover
    expect(target.querySelector('[role="dialog"]')).not.toBeNull();
    expect(mock.probe.projects.calls.some((c) => c.startsWith("vocabulary_promote"))).toBe(false);
  });

  it("the list changed while saving: said in plain words, the mark stays, focus back on the button", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    mock.probe.projects.failPromote("conflict");
    const pop = await popoverFor("roadmap");
    button("Make official in workspace-vocab", pop)!.click();
    flushSync();
    await tick();
    button("Make official", pop)!.click();
    await until(() => notes().includes("The official list changed while PerMark"), "said");
    expect(statusOf("roadmap")).toBe("unlisted");
    await until(
      () => document.activeElement?.textContent?.trim() === "Make official in workspace-vocab",
      "focus back",
    );
  });

  it("a read-only .permark folder: the system's refusal is said, nothing changes", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    mock.probe.projects.failPromote("io");
    const pop = await popoverFor("lab");
    button("Make official in workspace-vocab", pop)!.click();
    flushSync();
    await tick();
    button("Make official", pop)!.click();
    await until(() => windowStore.notices.length > 0, "said");
    expect(statusOf("lab")).toBe("unlisted");
    expect(projects.announcement).toBe("");
  });

  it("an official tag whose look the list sets: no pickers that would change nothing, the reason instead", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    const pop = await popoverFor("spec");
    expect(pop.textContent).toContain(
      "Its color and icon come from the official list of workspace-vocab.",
    );
    expect(pop.querySelector(".swatch")).toBeNull();
    expect(button("Make official in workspace-vocab", pop)).toBeUndefined();
  });

  it("a folder whose list can't be read: the button is off and says why", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    const pop = await popoverFor("lab");
    projects.served = {
      ...projects.served!,
      vocabulary: {
        ...projects.served!.vocabulary,
        file: { status: "unreadable", line: 3, message: "x" },
      },
    };
    flushSync();
    const off = button("Make official in workspace-vocab", pop)!;
    expect(off.disabled).toBe(true);
    expect(off.title).toBe(
      "The official list in this folder can't be read (line 3). PerMark won't change it until it is fixed.",
    );
  });

  it("a folder with included projects and no official list yet: promote starts the list (it says it creates it)", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    mock.probe.project.add("notes.md", "---\ntags: [idea]\n---\n# Notes\n");
    await tags.load();
    show(TagsPanel);
    await until(() => chip("idea") !== null, "chip");
    // dormant: no marks yet
    expect(statusOf("idea")).toBeNull();
    chip("idea")!.parentElement!.querySelector<HTMLButtonElement>("button.icon")!.click();
    flushSync();
    const pop = target.querySelector<HTMLElement>('[role="dialog"]')!;
    const name = `Make official in ${projects.rootName}`;
    button(name, pop)!.click();
    flushSync();
    await tick();
    expect(pop.textContent).toContain("PerMark creates .permark/vocabulary.toml in this folder.");
    button("Make official", pop)!.click();
    await until(() => statusOf("idea") === "official", "the list exists, marks show");
  });
});

describe("Settings > Projects inside: categories, official lists, Tags row", () => {
  async function settings(): Promise<void> {
    await openFolder(MOCK_PROJECTS_VOCAB);
    show(ProjectsSection);
    await until(() => target.querySelector('[data-project="hub-b"]') !== null, "rows");
  }

  async function expandRow(path: string): Promise<HTMLElement> {
    const row = target.querySelector<HTMLElement>(`[data-project="${path}"]`)!;
    row.querySelector<HTMLButtonElement>("button.rules")!.click();
    flushSync();
    await until(() => row.querySelector(".rules-body") !== null, "expanded");
    return row;
  }

  it("category field: official names offered while typing; its mark; local | unlisted can be made official", async () => {
    await settings();
    const hub = await expandRow("hub-b");
    const field = hub.querySelector<HTMLInputElement>('input[id$="-category"]')!;
    const list = document.getElementById(field.getAttribute("list") ?? "")!;
    expect(Array.from(list.querySelectorAll("option")).map((o) => o.value)).toEqual(["software"]);
    expect(hub.querySelector(".category-status")?.textContent).toContain("Official");
    expect(button("Make official in workspace-vocab", hub)).toBeUndefined();

    const site = await expandRow("site");
    expect(site.querySelector(".category-status")?.textContent).toContain("Local to site");
    const tools = await expandRow("tools/scratch");
    expect(tools.querySelector(".category-status")?.textContent).toContain("Unlisted (in no list)");
    button("Make official in workspace-vocab", tools)!.click();
    flushSync();
    await tick();
    expect(tools.textContent).toContain(
      "“experiments” becomes an official category for every project in workspace-vocab.",
    );
    button("Make official", tools)!.click();
    await until(
      () => mock.probe.projects.calls.includes("vocabulary_promote category experiments"),
      "sent",
    );
    await until(
      () => (tools.querySelector(".category-status")?.textContent ?? "").includes("Official"),
      "mark",
    );
  });

  it("a free category name is kept as typed (no list needed)", async () => {
    await settings();
    const tools = await expandRow("tools/scratch");
    const field = tools.querySelector<HTMLInputElement>('input[id$="-category"]')!;
    field.value = "Field Notes";
    field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await until(
      () =>
        mock.probe.projects.calls.some(
          (c) => c.startsWith("projects_card_set") && c.includes("Field Notes"),
        ),
      "saved as typed",
    );
  });

  it("Tags row: the project's own names that count as local", async () => {
    await settings();
    const hub = await expandRow("hub-b");
    expect(hub.textContent).toContain(
      "Official tags come from workspace-vocab. This project lists 1 tag of its own, shown as local.",
    );
  });

  it("official lists: names with their look; add one (said where), a name already there is said, nothing sent", async () => {
    await settings();
    const block = target.querySelector<HTMLElement>("[data-vocabulary]")!;
    expect(block).not.toBeNull();
    expect(block.textContent).toContain("needs-review");
    expect(block.textContent).toContain("software");
    expect(block.textContent).toContain(
      "PerMark saves the official list in this folder (.permark/vocabulary.toml).",
    );
    const field = block.querySelector<HTMLInputElement>('input[aria-label="New official tag"]')!;
    const add = field.parentElement!.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    expect(add.disabled).toBe(true);
    expect(add.title).toBe("Type a name first.");
    type(field, "spec");
    add.click();
    flushSync();
    await tick();
    expect(block.textContent).toContain("“spec” is already official.");
    expect(mock.probe.projects.calls.some((c) => c.startsWith("vocabulary_promote"))).toBe(false);
    type(field, "  Roadmap Items ");
    add.click();
    await until(
      () => mock.probe.projects.calls.includes("vocabulary_promote tag roadmap-items"),
      "sent",
    );
    await until(
      () => block.querySelector('[data-official="tag:roadmap-items"]') !== null,
      "listed",
    );
    expect(field.value).toBe("");
    expect(document.activeElement).toBe(field);
  });

  it("an unreadable official list: said on top, adding off with the reason; the file can be opened", async () => {
    await settings();
    projects.served = {
      ...projects.served!,
      vocabulary: {
        ...projects.served!.vocabulary,
        file: { status: "unreadable", line: 2, message: "conflict marker" },
      },
    };
    flushSync();
    const block = target.querySelector<HTMLElement>("[data-vocabulary]")!;
    expect(block.querySelector('[role="alert"]')?.textContent).toContain("(line 2)");
    for (const input of block.querySelectorAll<HTMLInputElement>("input")) {
      expect(input.disabled).toBe(true);
    }
    expect(button("Show the file", block)).toBeDefined();
  });

  it("a folder with found projects but none included (no .permark yet): no official lists block", async () => {
    await openFolder("C:/Users/demo/workspace");
    show(ProjectsSection);
    await until(() => projects.view?.ready === true, "view");
    flushSync();
    expect(target.querySelector("[data-vocabulary]")).toBeNull();
  });
});

describe("the detail card", () => {
  it("category chip with its mark in the official look; counts line joins plugin counts (open comments)", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    await until(() => projects.memberAt("hub-b") !== null, "member");
    const remove = contributions.register({
      owner: "comments",
      requires: NO_REQUIREMENT,
      kind: "projectCount",
      id: "test.count",
      count: (row) => (row.path === "hub-b" ? { text: "2 open comments", open: () => {} } : null),
    });
    cleanups.push(remove);
    show(ProjectCard, {
      row: projects.memberAt("hub-b")!,
      anchor: new DOMRect(),
      from: "hover",
      close: () => {},
      openTab: () => {},
      openSettings: () => {},
      hovering: () => {},
    });
    const card = target.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(card.querySelector(".chip [data-status]")?.getAttribute("data-status")).toBe("official");
    expect(card.querySelector(".chip")?.classList.contains("tinted")).toBe(true);
    expect(card.textContent).toContain("2 open comments");
  });

  it("comments: open ones below a project folder, said in words (none: nothing)", () => {
    comments.all = [
      { path: "hub-b/a.md", done: false },
      { path: "hub-b/x/b.md", done: false },
      { path: "hub-b/c.md", done: true },
      { path: "hub-bc/d.md", done: false },
    ] as unknown as typeof comments.all;
    expect(comments.projectCount("hub-b")).toBe("2 open comments");
    expect(comments.projectCount("site")).toBeNull();
    comments.all = [];
  });
});
