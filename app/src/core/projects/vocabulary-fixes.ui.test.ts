// @vitest-environment jsdom
// UX review round 4 (`permark-ux-reviews/ux-review-2026-10-22-projects-round4.md`) as tests on the mock backend
// (`workspace-vocab`: official `needs-review`, `spec` (blue), category `software`; hub-b lists `draft` + `spec`; site
// lists category `website`): "Make official" visible on the rows (F1), a legend of the marks (F2), the official lists
// edited in Settings (F3, F14), focus never dropped to the page (F4, F6), the look popover from the keyboard (F5, F7),
// counts on the detail card leading to the project's items (F8), official tags no file uses (F9), chip names (F12).
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER } from "../../api/mock";
import { MOCK_PROJECTS_VOCAB } from "../../api/mockProjects";

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
const { tags } = await import("../../plugins/tags/store.svelte");
const { comments } = await import("../../plugins/comments/store.svelte");
const { default: CommentsPanel } = await import("../../plugins/comments/CommentsPanel.svelte");
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

async function openFolder(root: string): Promise<void> {
  await mock.openPath(root, "thisTab");
  if (!started) {
    await windowStore.start();
    started = true;
  }
  conditions.set({ ...neutralContext(), workspace: "project", plugins: ["tags"] });
  cleanups.push(await projects.start(root));
  cleanups.push(tags.start());
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
const named = (name: string, scope: ParentNode = target): HTMLButtonElement | null =>
  scope.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`);
const chip = (name: string): HTMLElement | null =>
  target.querySelector<HTMLElement>(`.chip[data-tag="${name}"]`);
const row = (name: string): HTMLElement =>
  target.querySelector<HTMLElement>(`li[data-row="${name}"]`)!;
const statusOf = (name: string): string | null =>
  chip(name)?.querySelector("[data-status]")?.getAttribute("data-status") ?? null;
const active = (): Element | null => document.activeElement;
/** Waits until focus is on the control with this `data-act`. */
const focusOn = (act: string): Promise<void> =>
  until(() => active()?.getAttribute("data-act") === act, `focus on ${act}`);
/** What a screen reader hears of an element: its text without the parts hidden from it. */
function spoken(el: Element): string {
  const copy = el.cloneNode(true) as Element;
  for (const hidden of copy.querySelectorAll('[aria-hidden="true"]')) hidden.remove();
  return (copy.textContent ?? "").replace(/\s+/g, " ").trim();
}

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
  ui.closeSettingsPage();
  ui.showSidebar("files");
  documents.setWorkspace(null);
  vi.restoreAllMocks();
});

describe("Tags tab: status in words, Make official on the row (F1, F2, F9, F12)", () => {
  it("a legend names the three marks; each local | unlisted row says Make official, an official one doesn't", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    show(TagsPanel);
    await until(() => statusOf("roadmap") === "unlisted", "marks");
    const legend = target.querySelector<HTMLElement>('ul[aria-label="What the marks mean"]')!;
    expect(spoken(legend)).toContain("Official");
    expect(spoken(legend)).toContain("Local to one project");
    expect(spoken(legend)).toContain("Unlisted (only in files)");
    expect(legend.querySelectorAll("[data-status]")).toHaveLength(3);
    // marks large enough to tell apart
    expect(chip("spec")!.querySelector("[data-status] svg")?.getAttribute("width")).toBe("14");
    expect(button("Make official", row("roadmap"))).toBeDefined();
    expect(named("Make official: draft", row("draft"))).not.toBeNull();
    expect(button("Make official", row("spec"))).toBeUndefined();
  });

  it("the row's Make official asks in place, then the tag is official and focus is on it", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    show(TagsPanel);
    await until(() => statusOf("roadmap") === "unlisted", "marks");
    button("Make official", row("roadmap"))!.click();
    flushSync();
    await tick();
    expect(row("roadmap").textContent).toContain(
      "“roadmap” becomes an official tag for every project in workspace-vocab.",
    );
    await until(() => active()?.textContent?.trim() === "Make official", "on confirm");
    (active() as HTMLButtonElement).click();
    await until(() => statusOf("roadmap") === "official", "official");
    await until(() => active() === row("roadmap").querySelector(".chip button.face"), "on the tag");
    expect(button("Make official", row("roadmap"))).toBeUndefined();
  });

  it("official tags no file uses yet are listed (count 0); a plain folder has no legend", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    show(TagsPanel);
    await until(() => chip("needs-review") !== null, "unused official tag listed");
    expect(statusOf("needs-review")).toBe("official");
    expect(chip("needs-review")!.querySelector(".count")?.textContent).toBe("0");
    cleanups.pop()?.();
    for (const clean of cleanups.reverse()) clean();
    cleanups = [];
    target.replaceChildren();
    await openFolder(MOCK_FOLDER);
    show(TagsPanel);
    await until(() => target.querySelectorAll(".list .chip").length > 0, "chips");
    expect(target.querySelector('ul[aria-label="What the marks mean"]')).toBeNull();
    expect(button("Make official")).toBeUndefined();
  });

  it("a chip's spoken name: tag, status, count with its unit, no stray comma", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    show(TagsPanel);
    await until(() => statusOf("spec") === "official", "marks");
    const face = chip("spec")!.querySelector("button.face")!;
    const n = tags.overview!.tags.find((c) => c.name === "spec")!.count;
    // the name's box holds the spoken rest (one phrase, a unit with the count); the visible count is hidden from it
    expect(spoken(face)).toBe(`spec, Official, ${n} files`);
    expect(face.getAttribute("aria-label")).toBeNull();
    expect(spoken(chip("roadmap")!.querySelector("button.face")!)).toBe(
      "roadmap, Unlisted (found in files, in no list), 1 file",
    );
    // the legend's words are written out; its marks say nothing more
    expect(spoken(target.querySelector('ul[aria-label="What the marks mean"]')!)).toBe(
      "Official Local to one project Unlisted (only in files)",
    );
  });
});

describe("the look popover from the keyboard (F5, F6, F7)", () => {
  async function open(name: string): Promise<HTMLElement> {
    show(TagsPanel);
    await until(() => statusOf(name) !== null, "marks");
    row(name).querySelector<HTMLButtonElement>("button.icon")!.click();
    flushSync();
    await tick();
    return target.querySelector<HTMLElement>('[role="dialog"]')!;
  }

  it("opens with focus inside, its status on top; Esc closes it and focus is back on its button", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    const pop = await open("lab");
    expect(pop.contains(active())).toBe(true);
    expect(active()?.textContent?.trim()).toBe("Make official in workspace-vocab");
    expect(pop.querySelector(".status")?.textContent).toContain(
      "Unlisted (found in files, in no list)",
    );
    press(active()!, "Escape");
    await until(() => target.querySelector('[role="dialog"]') === null, "closed");
    await until(() => active() === row("lab").querySelector("button.icon"), "back on its button");
  });

  it("Esc on its opener closes it too", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    await open("lab");
    const opener = row("lab").querySelector<HTMLButtonElement>("button.icon")!;
    opener.focus();
    press(opener, "Escape");
    expect(target.querySelector('[role="dialog"]')).toBeNull();
    expect(opener.getAttribute("aria-expanded")).toBe("false");
  });

  it("after Make official in it, Esc closes it and focus is back on its button (not the page)", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    const pop = await open("roadmap");
    button("Make official in workspace-vocab", pop)!.click();
    flushSync();
    await until(() => active()?.textContent?.trim() === "Make official", "asked");
    (active() as HTMLButtonElement).click();
    await until(() => statusOf("roadmap") === "official", "official");
    await until(() => pop.contains(active()), "focus stays in the popover");
    press(active()!, "Escape");
    await until(
      () => active() === row("roadmap").querySelector("button.icon"),
      "back on its button",
    );
  });

  it("an official tag: a way out to the official list (Settings at that name's look)", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    const pop = await open("spec");
    const edit = button("Edit the official list…", pop)!;
    expect(active()).toBe(edit);
    edit.click();
    flushSync();
    expect(ui.settingsPage).toEqual({ status: "open", section: "projects" });
    expect(projects.ask).toMatchObject({ kind: "vocabulary", of: "tag", name: "spec" });
    expect(target.querySelector('[role="dialog"]')).toBeNull();
    // the official lists take the ask: that name's look is open, focus on its first choice
    show(ProjectsSection);
    await until(() => projects.ask === null, "ask taken");
    const entry = target.querySelector<HTMLElement>('[data-official="tag:spec"]')!;
    await until(
      () => entry.querySelector('[data-act="look-panel"]')?.contains(active()) === true,
      "focus",
    );
  });
});

describe("Settings: the official lists edited in the app (F3, F14)", () => {
  async function lists(): Promise<HTMLElement> {
    await openFolder(MOCK_PROJECTS_VOCAB);
    show(ProjectsSection);
    await until(() => target.querySelector("[data-vocabulary]") !== null, "lists");
    return target.querySelector<HTMLElement>("[data-vocabulary]")!;
  }
  const entry = (block: HTMLElement, key: string): HTMLElement | null =>
    block.querySelector<HTMLElement>(`[data-official="${key}"]`);

  it("says that files keep their tags; each name has Rename, Color and icon, Remove", async () => {
    const block = await lists();
    expect(block.textContent).toContain("files and project cards keep the names they use");
    for (const key of ["tag:spec", "tag:needs-review", "category:software"]) {
      const e = entry(block, key)!;
      expect(button("Rename", e)).toBeDefined();
      expect(button("Color and icon", e)).toBeDefined();
      expect(button("Remove", e)).toBeDefined();
    }
    expect(named("Rename spec", block)).not.toBeNull();
    // the file stays one way among others
    expect(button("Show the file", block)).toBeDefined();
  });

  it("Rename in place: Enter saves (typed name kept by the tag rules), said, focus on the renamed name", async () => {
    const block = await lists();
    button("Rename", entry(block, "tag:needs-review")!)!.click();
    await focusOn("new-name");
    const input = active() as HTMLInputElement;
    expect(input.getAttribute("aria-label")).toBe("New name for needs-review");
    expect(input.value).toBe("needs-review");
    input.value = "To Review";
    press(input, "Enter");
    await until(
      () => mock.probe.projects.calls.includes("vocabulary_rename tag needs-review to-review"),
      "sent",
    );
    await until(() => entry(block, "tag:to-review") !== null, "renamed");
    expect(entry(block, "tag:needs-review")).toBeNull();
    await until(
      () => active() === entry(block, "tag:to-review")!.querySelector('[data-act="rename"]'),
      "focus",
    );
    await until(
      () => projects.announcement.includes("Files keep “needs-review” as it is."),
      "said",
    );
  });

  it("Rename: Esc cancels (the page stays), nothing sent; a name already official is refused in words", async () => {
    const block = await lists();
    button("Rename", entry(block, "tag:needs-review")!)!.click();
    await focusOn("new-name");
    const input = active() as HTMLInputElement;
    const esc = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    input.dispatchEvent(esc);
    flushSync();
    expect(esc.defaultPrevented).toBe(true);
    await until(
      () => active() === entry(block, "tag:needs-review")!.querySelector('[data-act="rename"]'),
      "back",
    );
    expect(mock.probe.projects.calls.some((c) => c.startsWith("vocabulary_rename"))).toBe(false);
    button("Rename", entry(block, "tag:needs-review")!)!.click();
    await focusOn("new-name");
    const again = active() as HTMLInputElement;
    again.value = "spec";
    press(again, "Enter");
    await until(
      () => JSON.stringify(windowStore.notices).includes("“spec” is already official."),
      "refused",
    );
    expect(entry(block, "tag:needs-review")).not.toBeNull();
  });

  it("Remove asks first, in place; confirm removes it, focus on the next name; Cancel | Esc keeps it", async () => {
    const block = await lists();
    button("Remove", entry(block, "tag:needs-review")!)!.click();
    await focusOn("confirm-remove");
    const ask = entry(block, "tag:needs-review")!.querySelector<HTMLElement>('[role="group"]')!;
    expect(ask.textContent).toContain(
      "Remove “needs-review” from the official tags? Files keep this tag as it is.",
    );
    expect(active()?.getAttribute("data-act")).toBe("confirm-remove");
    press(active()!, "Escape");
    await until(
      () => active() === entry(block, "tag:needs-review")!.querySelector('[data-act="remove"]'),
      "kept",
    );
    expect(mock.probe.projects.calls.some((c) => c.startsWith("vocabulary_remove"))).toBe(false);

    button("Remove", entry(block, "tag:needs-review")!)!.click();
    await focusOn("confirm-remove");
    (active() as HTMLButtonElement).click();
    await until(() => entry(block, "tag:needs-review") === null, "removed");
    expect(mock.probe.projects.calls).toContain("vocabulary_remove tag needs-review");
    await until(
      () => active() === entry(block, "tag:spec")!.querySelector('[data-act="rename"]'),
      "next name",
    );
    await until(
      () =>
        projects.announcement ===
        "“needs-review” is no longer an official tag. Files keep it as it is.",
      "said",
    );
  });

  it("Color and icon: pickers in place, applied as picked; Esc closes them, focus back", async () => {
    const block = await lists();
    const look = button("Color and icon", entry(block, "category:software")!)!;
    look.click();
    flushSync();
    expect(look.getAttribute("aria-expanded")).toBe("true");
    const panel = entry(block, "category:software")!.querySelector<HTMLElement>(
      '[data-act="look-panel"]',
    )!;
    await until(() => panel.contains(active()), "focus in the pickers");
    named("Purple", panel)!.click();
    await until(
      () =>
        mock.probe.projects.calls.some((c) =>
          c.startsWith("vocabulary_set_style category software purple"),
        ),
      "sent",
    );
    await until(() => projects.view?.vocabulary.categories[0]?.color === "purple", "applied");
    press(active()!, "Escape");
    await until(
      () => active() === entry(block, "category:software")!.querySelector('[data-act="look"]'),
      "back on its button",
    );
  });

  it("Add says how a typed name is kept, then 'Added' with Undo (= remove)", async () => {
    const block = await lists();
    const field = block.querySelector<HTMLInputElement>('input[aria-label="New official tag"]')!;
    type(field, "Release Notes");
    expect(block.textContent).toContain("Will be added as “release-notes”");
    field.form!.requestSubmit();
    await until(() => entry(block, "tag:release-notes") !== null, "added");
    expect(block.textContent).toContain("Added “release-notes”.");
    expect(active()).toBe(field);
    button("Undo", block)!.click();
    await until(() => entry(block, "tag:release-notes") === null, "undone");
    expect(mock.probe.projects.calls).toContain("vocabulary_remove tag release-notes");
    expect(block.textContent).toContain("Undone: “release-notes” is no longer official.");
    expect(button("Undo", block)).toBeUndefined();
  });

  it("the list can't be changed (read-only .permark): the refusal is said, the name stays", async () => {
    const block = await lists();
    mock.probe.projects.failPromote("io");
    button("Remove", entry(block, "tag:needs-review")!)!.click();
    await focusOn("confirm-remove");
    (active() as HTMLButtonElement).click();
    await until(() => windowStore.notices.length > 0, "said");
    expect(entry(block, "tag:needs-review")).not.toBeNull();
  });
});

describe("category field: focus kept, official names said (F4, F10)", () => {
  it("Tab out of an official category: Make official… goes, focus on the status words (never the page)", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    show(ProjectsSection);
    await until(() => target.querySelector('[data-project="tools/scratch"]') !== null, "rows");
    const tools = target.querySelector<HTMLElement>('[data-project="tools/scratch"]')!;
    tools.querySelector<HTMLButtonElement>("button.rules")!.click();
    flushSync();
    await until(() => tools.querySelector(".rules-body") !== null, "expanded");
    const field = tools.querySelector<HTMLInputElement>('input[id$="-category"]')!;
    const offered = document.getElementById(`${field.id.replace(/-category$/, "")}-offered`);
    expect(offered?.textContent).toContain("Official categories: software.");
    expect(field.getAttribute("aria-describedby")).toContain("-offered");
    field.focus();
    field.value = "software";
    // Tab: the next control is "Make official…" (the category is still unlisted while it saves)
    button("Make official in workspace-vocab", tools)!.focus();
    await until(
      () => (tools.querySelector(".category-status")?.textContent ?? "").includes("Official"),
      "official",
    );
    await until(() => active() === tools.querySelector(".category-status"), "focus on the status");
    expect(active()).not.toBe(document.body);
  });
});

describe("detail card counts lead to the project's items (F8)", () => {
  it("each count is a link: the card closes, the plugin shows that project's items", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    await until(() => projects.memberAt("hub-b") !== null, "member");
    let opened = "";
    cleanups.push(
      contributions.register({
        owner: "comments",
        requires: NO_REQUIREMENT,
        kind: "projectCount",
        id: "test.count",
        count: (r) => ({ text: "2 open comments", open: () => (opened = r.path) }),
      }),
    );
    let closed: boolean | null = null;
    show(ProjectCard, {
      row: projects.memberAt("hub-b")!,
      anchor: new DOMRect(),
      from: "menu",
      close: (refocus: boolean) => (closed = refocus),
      openTab: () => {},
      openSettings: () => {},
      hovering: () => {},
    });
    const card = target.querySelector<HTMLElement>('[role="dialog"]')!;
    // from the menu, focus still starts on the card's first action
    await until(() => active()?.textContent?.trim() === "Open in new tab", "first action");
    button("2 open comments", card)!.click();
    expect(closed).toBe(false);
    expect(opened).toBe("hub-b");
  });

  it("Comments tab for one project: only its comments, which project it is, Show all", async () => {
    await openFolder(MOCK_PROJECTS_VOCAB);
    const base = {
      text: "Check this",
      quote: "a passage",
      headingPath: [],
      line: 1,
      endLine: 1,
      createdMs: 0,
      state: "anchored" as const,
      done: false,
      doneMs: null,
      doneBy: null,
      reply: null,
      replies: [],
    };
    comments.all = [
      { ...base, id: "1", path: "hub-b/a.md" },
      { ...base, id: "2", path: "hub-bc/d.md" },
    ];
    comments.ready = true;
    comments.showOnly({ folder: "hub-b", name: "Hub B" });
    show(CommentsPanel);
    expect(target.textContent).toContain("Showing only Hub B");
    expect(target.querySelector('h3[title="hub-b/a.md"]')).not.toBeNull();
    expect(target.querySelector('h3[title="hub-bc/d.md"]')).toBeNull();
    // focus on the line saying which project (the person came for that list)
    await until(() => active()?.textContent === "Showing only Hub B", "focus");
    button("Show all")!.click();
    flushSync();
    expect(target.querySelector('h3[title="hub-bc/d.md"]')).not.toBeNull();
    expect(target.textContent).not.toContain("Showing only");
    comments.all = [];
  });
});
