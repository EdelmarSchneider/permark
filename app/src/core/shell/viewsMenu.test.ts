// @vitest-environment jsdom
// The Panels menu: every view the window offers, by the region that lists it, each on | off (present); views that
// cannot be closed say why; plugins off | dormant panels are not listed; a pick opens | closes the view. And the
// palette's "Show <name>" entries: one per view that can be closed, following the plugins on | off.
import { flushSync, tick } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Api, DocumentView } from "../../api";
import type { PluginId } from "../../api/generated/PluginId";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { createMockApi } = await import("../../api/mock");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { conditions, neutralContext } = await import("../conditions/context.svelte");
const { documents } = await import("../doc/document.svelte");
const { reading } = await import("../doc/reading.svelte");
const { en } = await import("../i18n/table");
const { ContributionRegistry } = await import("../plugins/registry.svelte");
const { contributions } = await import("../plugins/instance.svelte");
const { commandLabel, listCommands } = await import("../commands/commands");
const { buildKeymap } = await import("../commands/keymap");
const { windowStore } = await import("../window.svelte");
const { layout } = await import("./layout.svelte");
const { arrangement } = await import("./arrangement.svelte");
const { defaultArrangement, FILES, moveTo, OUTLINE, REGIONS, SEARCH } =
  await import("./arrangement");
const { ui } = await import("./ui.svelte");
const { registerViewCommands, viewsEntries } = await import("./viewsMenu.svelte");
const { selectView } = await import("./sides");

const LEFT = "unread.panel";
const LIVE = "live-feed.panel";
const view = () => import("../testing/StripFirst.svelte");

let api: Api;
let stop: () => void = () => {};
const removals: (() => void)[] = [];

beforeEach(async () => {
  api = createMockApi();
  stop = arrangement.start(api);
  conditions.set({ ...neutralContext(), workspace: "project" });
  await api.projectSessionSet("layout.views", null);
  await layout.load(api, true);
  flushSync();
});

afterEach(async () => {
  stop();
  for (const notice of closedNotices()) windowStore.dismiss(notice.id);
  windowStore.dismissKey("layout");
  for (const remove of removals.splice(0)) remove();
  conditions.set(neutralContext());
  documents.state = { status: "empty" };
  reading.setOutline(true);
  ui.resetFocus();
  arrangement.arrange(defaultArrangement());
  await layout.load(api, false);
});

/** A plugin's panel: a sidebar tab (left) or a panel with its own header (right). */
function panel(id: string, side: "left" | "right", offered: () => boolean = () => true): void {
  removals.push(
    contributions.register({
      kind: "panel",
      id,
      owner: (side === "left" ? "unread" : "live-feed") as PluginId,
      side,
      label: side === "left" ? "unread.title" : "live.label",
      requires: NO_REQUIREMENT,
      view,
      offered,
    }),
  );
  flushSync();
}

/** A document with a heading is shown (the outline has something to list). */
function showOutline(): void {
  const shown: DocumentView = {
    path: "a.md",
    name: "a.md",
    format: "markdown",
    role: null,
    body: {
      kind: "markdown",
      blocks: [],
      outline: [{ label: "Title", level: 1, id: "title", line: 1, block: "b1" }],
      frontMatter: null,
    },
    hash: "ab".repeat(8),
    size: 8,
    mtimeMs: 1,
    editable: { editable: "surgical" },
    notes: [],
  };
  documents.state = { status: "ready", view: shown, deleted: null, fresh: true };
  flushSync();
}

/** The menu as a person reads it: `#` a region, `[x]` | `[ ]` on | off, `(off)` cannot act, `—` between blocks. */
const menu = (): string[] =>
  viewsEntries().map((entry) => {
    switch (entry.kind) {
      case "separator":
        return "—";
      case "heading":
        return `# ${entry.label}`;
      case "note":
        return `(${entry.label})`;
      case "item": {
        const box = entry.checked === undefined ? "" : entry.checked ? "[x] " : "[ ] ";
        return `${box}${entry.label}${entry.disabled === true ? " (off)" : ""}`;
      }
      default:
        return entry satisfies never;
    }
  });

/** The entry of the view called `label`. */
function row(label: string) {
  const found = viewsEntries().find((e) => e.kind === "item" && e.label === label);
  if (found?.kind !== "item") throw new Error(`no entry "${label}"`);
  return found;
}

function closedNotices() {
  return windowStore.notices.flatMap((n) =>
    n.kind === "action" && n.key?.startsWith("view-closed") === true ? [n] : [],
  );
}

/** What the status line holds once it settled (it is emptied first and filled a tick later). */
async function said(): Promise<string> {
  await tick();
  await tick();
  return arrangement.status;
}

describe("the Panels menu", () => {
  it("lists every view by the region that lists it, each on | off, with the arrangement's entries last", () => {
    panel(LEFT, "left");
    panel(LIVE, "right");
    expect(menu()).toEqual([
      `# ${en["panel.sidebar"]}`,
      "[x] Files (off)",
      "[x] Search (off)",
      "[x] Unread",
      `# ${en["panel.right"]}`,
      "[ ] Outline (off)",
      "[ ] Live changes",
      `# ${en["panel.bottom"]}`,
      `(${en["panel.none"]})`,
      "—",
      "Layouts…",
      "Reset layout (off)",
    ]);
    // each view is its own on | off (a checkbox), not one choice of a set
    expect(row("Unread").checkbox).toBe(true);
    // what cannot act says why: Files and Search are always open, the outline needs headings
    expect(row("Files").why).toBe(en["panel.alwaysShown"]);
    expect(row("Search").why).toBe(en["panel.alwaysShown"]);
    expect(row("Outline").why).toBe(en["panel.noOutline"]);
    expect(row("Unread").why).toBeUndefined();
    showOutline();
    expect(row("Outline")).toMatchObject({ checked: true, disabled: false });
  });

  it("follows the arrangement: a view moved to the bottom is listed there", () => {
    panel(LIVE, "right");
    arrangement.arrange(moveTo(arrangement.layout, SEARCH, "bottom"));
    flushSync();
    const at = menu();
    expect(at.slice(at.indexOf(`# ${en["panel.bottom"]}`), at.indexOf("—"))).toEqual([
      `# ${en["panel.bottom"]}`,
      "[x] Search (off)",
    ]);
    expect(at).not.toContain(`(${en["panel.none"]})`);
  });

  it("lists nothing of a plugin that is off, nor a panel its plugin does not offer yet", () => {
    let offered = false;
    panel(LIVE, "right", () => offered);
    expect(menu()).not.toContain("[ ] Live changes");
    offered = true;
    // a fresh registration reads the plugin's offer again
    for (const remove of removals.splice(0)) remove();
    panel(LIVE, "right", () => offered);
    expect(menu()).toContain("[ ] Live changes");
    for (const remove of removals.splice(0)) remove();
    flushSync();
    expect(menu()).not.toContain("[ ] Live changes");
    // a choice the project still holds for it lists nothing either
    layout.setOpen(LIVE, true);
    expect(menu().some((line) => line.includes("Live changes"))).toBe(false);
  });

  it("outside a project lists only the views that window can show", () => {
    panel(LEFT, "left");
    panel(LIVE, "right");
    conditions.set({ ...neutralContext(), workspace: "singleFile" });
    flushSync();
    expect(menu().filter((line) => line.startsWith("["))).toEqual([
      "[ ] Outline (off)",
      "[ ] Live changes",
    ]);
  });

  it("a pick on a closed view opens it, shows its region, selects it and says so", async () => {
    panel(LIVE, "right");
    layout.setRightHidden(true);
    row("Live changes").run(false);
    flushSync();
    expect(arrangement.isPresent(LIVE)).toBe(true);
    expect(layout.rightHidden).toBe(false);
    expect(arrangement.isSelected(LIVE)).toBe(true);
    expect(row("Live changes").checked).toBe(true);
    expect(await said()).toBe("Live changes shown");
  });

  it("a pick on an open view closes it, with a notice of its own whose Undo brings it back", async () => {
    panel(LEFT, "left");
    panel(LIVE, "right");
    layout.setOpen(LIVE, true);
    row("Live changes").run(false);
    flushSync();
    expect(arrangement.isPresent(LIVE)).toBe(false);
    expect(await said()).toBe("Live changes closed");
    // a sidebar tab closes the same way, with its own notice: the first one keeps its Undo
    row("Unread").run(false);
    flushSync();
    expect(arrangement.isPresent(LEFT)).toBe(false);
    expect(closedNotices().map((n) => [n.text, n.actions.map((a) => a.label)])).toEqual([
      ["Live changes closed", [en["fileOps.undo"]]],
      ["Unread closed", [en["fileOps.undo"]]],
    ]);
    void closedNotices()[1]?.actions[0]?.run();
    flushSync();
    expect(arrangement.isPresent(LEFT)).toBe(true);
    expect(arrangement.isSelected(LEFT)).toBe(true);
  });

  it("Files and Search cannot be closed from it: their pick does nothing", () => {
    row("Files").run(false);
    flushSync();
    expect(arrangement.isPresent(FILES)).toBe(true);
    expect(closedNotices()).toEqual([]);
  });
});

describe("the palette's Show entries", () => {
  const KEYMAP = buildKeymap([], {}, []);
  let registry: InstanceType<typeof ContributionRegistry>;
  let off: () => void = () => {};

  beforeEach(() => {
    registry = new ContributionRegistry();
    off = registerViewCommands(registry);
    flushSync();
  });

  afterEach(() => off());

  /** The palette's Show entries offered now, as their labels read. */
  const shows = (): string[] =>
    listCommands(registry.items, conditions.current, KEYMAP, { palette: true })
      .filter((entry) => entry.command.id.startsWith("view.show."))
      .map((entry) => commandLabel(entry.command));

  it("one per view that can be closed, named after it; none for Files and Search", () => {
    panel(LEFT, "left");
    panel(LIVE, "right");
    expect(shows().sort()).toEqual(["Show Live changes", "Show Unread"]);
    // the outline: once the document shown has one, while it is not on screen (its region hidden)
    showOutline();
    expect(shows().sort()).toEqual(["Show Live changes", "Show Unread"]);
    layout.setRightHidden(true);
    flushSync();
    expect(shows().sort()).toEqual(["Show Live changes", "Show Outline", "Show Unread"]);
    const ids = registry.of("command").map((c) => c.id);
    expect(ids).not.toContain(`view.show.${FILES}`);
    expect(ids).not.toContain(`view.show.${SEARCH}`);
  });

  it("follows the plugins: off | dormant, no entry; offered again, the entry is back", () => {
    let offered = false;
    panel(LIVE, "right", () => offered);
    expect(registry.of("command").map((c) => c.id)).not.toContain(`view.show.${LIVE}`);
    offered = true;
    for (const remove of removals.splice(0)) remove();
    panel(LIVE, "right", () => offered);
    expect(registry.of("command").map((c) => c.id)).toContain(`view.show.${LIVE}`);
    for (const remove of removals.splice(0)) remove();
    flushSync();
    expect(registry.of("command").map((c) => c.id)).not.toContain(`view.show.${LIVE}`);
  });

  it("a view on screen has none; a tab its group does not show, or a view of a hidden region, has one", () => {
    panel(LEFT, "left");
    panel(LIVE, "right");
    expect(shows().sort()).toEqual(["Show Live changes", "Show Unread"]);
    selectView(LEFT);
    layout.setOpen(LIVE, true);
    flushSync();
    expect(arrangement.onScreen(LEFT)).toBe(true);
    expect(shows()).toEqual([]);
    selectView(FILES);
    layout.setRightHidden(true);
    flushSync();
    expect(shows().sort()).toEqual(["Show Live changes", "Show Unread"]);
  });

  it("a sidebar tab's entry needs a project", () => {
    panel(LEFT, "left");
    conditions.set({ ...neutralContext(), workspace: "singleFile" });
    flushSync();
    expect(shows()).toEqual([]);
  });

  it("runs as the menu does: the view shown and selected, its region shown", () => {
    panel(LIVE, "right");
    layout.setRightHidden(true);
    registry
      .of("command")
      .find((c) => c.id === `view.show.${LIVE}`)
      ?.run();
    flushSync();
    expect(arrangement.isPresent(LIVE)).toBe(true);
    expect(arrangement.onScreen(LIVE)).toBe(true);
  });

  it("goes with the core's commands: nothing left once removed", () => {
    panel(LIVE, "right");
    expect(registry.of("command").length).toBeGreaterThan(0);
    off();
    expect(registry.items).toEqual([]);
    // a view offered later registers nothing
    panel(LEFT, "left");
    expect(registry.items).toEqual([]);
  });

  it("keeps one entry per view while the views change around it", () => {
    panel(LIVE, "right");
    panel(LEFT, "left");
    for (const region of REGIONS) arrangement.arrange(moveTo(arrangement.layout, LIVE, region));
    flushSync();
    const ids = registry.of("command").map((c) => c.id);
    expect(ids.filter((id) => id === `view.show.${LIVE}`)).toHaveLength(1);
    expect(ids).toContain(`view.show.${OUTLINE}`);
  });
});
