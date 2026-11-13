// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DocumentView, VersionRow } from "../../api";
import type { VersionsProblem, VersionsUi } from "./store.svelte";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { documents } = await import("../../core/doc/document.svelte");
const { language } = await import("../../core/i18n");
const { exactTime } = await import("../../core/time");
const { review } = await import("./store.svelte");
const { default: VersionsHost } = await import("./VersionsHost.svelte");

const HOUR = 3_600_000;
const GUIDE = "docs/guide.md";
const LONG =
  "Restructure the page into install, first steps and troubleshooting so that new readers find the setup notes without searching all of it";

type Open = Extract<VersionsUi, { status: "open" }>;

/** Version `n` of the list (1 = the newest), made `2 * n` hours ago. */
function row(n: number, more: Partial<VersionRow> = {}): VersionRow {
  return {
    commit: n.toString(16).padStart(40, "0"),
    atMs: Date.now() - n * 2 * HOUR,
    author: "Ana Souza",
    coAuthors: [],
    subject: `Change ${n}`,
    same: false,
    ...more,
  };
}

function view(path: string): DocumentView {
  return {
    path,
    name: path.split("/").at(-1) ?? path,
    format: "text",
    role: null,
    body: { kind: "text", text: "text\n", language: "md" },
    hash: "ab".repeat(8),
    size: 5,
    mtimeMs: 1,
    editable: { editable: "surgical" },
    notes: [],
  };
}

/** The page shows `path`. */
function shows(path: string): void {
  documents.state = { status: "ready", view: view(path), deleted: null, fresh: true };
  flushSync();
}

/** An open list of the guide, as given. */
function listOf(more: Partial<Open> = {}): Open {
  return {
    status: "open",
    path: GUIDE,
    loading: false,
    rows: [],
    next: null,
    renamedFrom: null,
    cut: false,
    problem: null,
    note: null,
    ...more,
  };
}

/** The list is open as given, and drawn. */
function open(more: Partial<Open> = {}): void {
  review.versions = listOf(more);
  flushSync();
}

let target: HTMLElement;
let opener: HTMLButtonElement;
let component: Record<string, unknown> | null = null;

beforeEach(() => {
  document.body.replaceChildren();
  opener = document.createElement("button");
  target = document.createElement("div");
  document.body.append(opener, target);
  opener.focus();
  review.versions = { status: "closed" };
  shows(GUIDE);
  component = mount(VersionsHost, { target });
  flushSync();
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  review.versions = { status: "closed" };
  documents.state = { status: "empty" };
  vi.restoreAllMocks();
});

const dialog = (): HTMLElement | null => target.querySelector<HTMLElement>('[role="dialog"]');
const listbox = (): HTMLElement | null => target.querySelector<HTMLElement>('[role="listbox"]');
const options = (): HTMLElement[] =>
  Array.from(target.querySelectorAll<HTMLElement>('[role="option"]'));
const button = (action: string): HTMLButtonElement | null =>
  target.querySelector<HTMLButtonElement>(`button[data-action="${action}"]`);
/** Index of the option the keyboard is on. */
const active = (): number =>
  options().findIndex((o) => o.id === listbox()?.getAttribute("aria-activedescendant"));

function key(el: HTMLElement | null, name: string, init: KeyboardEventInit = {}): void {
  el?.dispatchEvent(
    new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true, ...init }),
  );
  flushSync();
}

describe("the list of earlier versions", () => {
  it("draws nothing until the person asks for it", () => {
    expect(target.querySelector('[role="dialog"], [role="listbox"], button')).toBeNull();
    expect(target.textContent?.trim()).toBe("");
  });

  it("is a dialog named after the file, not its folders", () => {
    open({ rows: [row(1)] });
    expect(dialog()?.getAttribute("aria-modal")).toBe("true");
    const title = document.getElementById(dialog()?.getAttribute("aria-labelledby") ?? "");
    expect(title?.textContent).toBe("Earlier versions of guide.md");
  });

  it("says it is reading while the first rows are on their way", () => {
    open({ loading: true });
    expect(target.querySelector('[aria-busy="true"]')?.textContent).toContain(
      "Reading the history…",
    );
    expect(listbox()).toBeNull();
    open({ rows: [row(1)] });
    expect(target.querySelector('[aria-busy="true"]')).toBeNull();
    expect(target.textContent).not.toContain("Reading the history…");
  });

  it("a row says when, what, the exact moment and who", () => {
    const rows = [row(1, { subject: "Rewrite the install steps" }), row(2, { subject: LONG })];
    open({ rows });
    expect(listbox()?.getAttribute("aria-label")).toBe("Versions");
    const [first, second] = options();
    expect(first?.querySelector(".when")?.textContent).toBe("2 hours ago");
    expect(first?.querySelector(".subject")?.textContent).toBe("Rewrite the install steps");
    expect(first?.querySelector(".exact")?.textContent).toBe(
      exactTime(rows[0]!.atMs, language.locale),
    );
    expect(first?.querySelector(".who")?.textContent).toBe("Ana Souza");
    // a long subject is cut by the row, never by the text: the whole of it is in the tooltip
    expect(second?.querySelector(".when")?.textContent).toBe("4 hours ago");
    expect(second?.querySelector(".subject")?.textContent).toBe(LONG);
    expect(second?.querySelector(".subject")?.getAttribute("title")).toBe(LONG);
  });

  it("names the co-authors after the author, as a list in the language of the interface", () => {
    open({
      rows: [row(1, { coAuthors: ["Rafael Lima"] }), row(2, { coAuthors: ["Rafael Lima", "Lin"] })],
    });
    expect(options().map((o) => o.querySelector(".who")?.textContent)).toEqual([
      "Ana Souza with Rafael Lima",
      "Ana Souza with Rafael Lima and Lin",
    ]);
  });

  it("shows text from the repository as text: never markup, never a link", () => {
    open({
      rows: [
        row(1, {
          subject: "<b>bold</b> [x](https://example.com)",
          author: "<i>Ana</i>",
          coAuthors: ["<img src=x onerror=alert(1)>"],
        }),
      ],
      renamedFrom: "<u>old</u>.md",
    });
    const option = options()[0];
    expect(option?.querySelector(".subject")?.textContent).toBe(
      "<b>bold</b> [x](https://example.com)",
    );
    expect(option?.querySelector(".who")?.textContent).toBe(
      "<i>Ana</i> with <img src=x onerror=alert(1)>",
    );
    expect(dialog()?.querySelector("b, i, u, img, a")).toBeNull();
    expect(dialog()?.textContent).toContain("<u>old</u>.md");
  });

  it("a click or Enter | Space on a row compares with that version", () => {
    const pick = vi.spyOn(review, "pickVersion").mockResolvedValue();
    const rows = [row(1), row(2), row(3)];
    open({ rows });
    options()[1]?.click();
    expect(pick).toHaveBeenLastCalledWith(rows[1]);
    key(listbox(), "ArrowDown");
    key(listbox(), "Enter");
    expect(pick).toHaveBeenLastCalledWith(rows[2]);
    key(listbox(), "Home");
    key(listbox(), " ");
    expect(pick).toHaveBeenLastCalledWith(rows[0]);
    expect(pick).toHaveBeenCalledTimes(3);
  });

  it("the version with the text of now says so and cannot be picked", () => {
    const pick = vi.spyOn(review, "pickVersion").mockResolvedValue();
    open({ rows: [row(1, { same: true }), row(2)] });
    const [same, other] = options();
    expect(same?.getAttribute("aria-disabled")).toBe("true");
    expect(same?.querySelector(".same")?.textContent).toBe("Same text as now");
    expect(other?.getAttribute("aria-disabled")).toBeNull();
    expect(other?.querySelector(".same")).toBeNull();
    same?.click();
    key(listbox(), "Enter");
    expect(pick).not.toHaveBeenCalled();
  });

  it("no row can be picked while the page still shows another document", () => {
    const pick = vi.spyOn(review, "pickVersion").mockResolvedValue();
    // the window is on its way to the guide: what is on screen is still the file before
    documents.state = { status: "loading", path: GUIDE, view: view("README.md") };
    const rows = [row(1), row(2)];
    open({ rows });
    expect(options().map((o) => o.getAttribute("aria-disabled"))).toEqual(["true", "true"]);
    options()[0]?.click();
    key(listbox(), "Enter");
    expect(pick).not.toHaveBeenCalled();
    shows(GUIDE);
    expect(options().map((o) => o.getAttribute("aria-disabled"))).toEqual([null, null]);
    options()[0]?.click();
    expect(pick).toHaveBeenCalledWith(rows[0]);
  });

  it("the arrows, Home and End move through the rows from one Tab stop, and stop at the ends", () => {
    open({ rows: [row(1), row(2), row(3)] });
    expect(listbox()?.tabIndex).toBe(0);
    expect(options().map((o) => o.tabIndex)).toEqual([-1, -1, -1]);
    expect(active()).toBe(0);
    expect(options().map((o) => o.getAttribute("aria-selected"))).toEqual([
      "true",
      "false",
      "false",
    ]);
    key(listbox(), "ArrowUp");
    expect(active()).toBe(0);
    key(listbox(), "ArrowDown");
    expect(active()).toBe(1);
    key(listbox(), "End");
    expect(active()).toBe(2);
    key(listbox(), "ArrowDown");
    expect(active()).toBe(2);
    key(listbox(), "ArrowUp");
    expect(active()).toBe(1);
    key(listbox(), "Home");
    expect(active()).toBe(0);
    // a modified arrow is not a move (Alt+Left | Alt+Up belong to the window)
    key(listbox(), "ArrowDown", { altKey: true });
    expect(active()).toBe(0);
    expect(options()[0]?.getAttribute("aria-selected")).toBe("true");
  });

  it("Show more reads the next rows, and waits while a page is on its way", () => {
    const more = vi.spyOn(review, "moreVersions").mockResolvedValue();
    open({ rows: [row(1)], next: null });
    expect(button("more")).toBeNull();
    open({ rows: [row(1)], next: 50 });
    expect(button("more")?.textContent?.trim()).toBe("Show more");
    button("more")?.click();
    expect(more).toHaveBeenCalledTimes(1);
    open({ rows: [row(1)], next: 50, loading: true });
    expect(button("more")?.disabled).toBe(true);
    // the rows read so far stay while the next ones come
    expect(options()).toHaveLength(1);
  });

  it("the keyboard stays in the list after Show more, on the first row that came", async () => {
    const first = row(1);
    vi.spyOn(review, "moreVersions").mockImplementation(async () => {
      await tick();
      review.versions = listOf({ rows: [first, row(2), row(3)], next: null });
    });
    open({ rows: [first], next: 50 });
    button("more")?.focus();
    button("more")?.click();
    await vi.waitFor(() => expect(active()).toBe(1));
    // the button went with the end of the history: the keyboard did not go with it
    expect(button("more")).toBeNull();
    expect(document.activeElement).toBe(listbox());
  });

  it("says why there is no list, with Try again only when reading again can help", () => {
    const retry = vi.spyOn(review, "retryVersions").mockResolvedValue();
    const problems: [VersionsProblem, string, boolean][] = [
      [{ kind: "unavailable" }, "Earlier versions are not available here.", false],
      [{ kind: "notCommitted" }, "This file has not been committed to git yet.", false],
      [{ kind: "onlyCurrent" }, "There is only one version: the one you see.", false],
      [
        { kind: "otherRepository" },
        "This file belongs to another repository inside this folder.",
        false,
      ],
      [{ kind: "unsupported" }, "This file is not text, so versions cannot be compared.", false],
      [{ kind: "unsaved" }, "Save your changes first to compare with earlier versions.", false],
      [{ kind: "git", text: "Git is not available." }, "Git is not available.", false],
      [{ kind: "slow" }, "Reading the history took too long.", true],
      [{ kind: "failed" }, "The history could not be read.", true],
    ];
    for (const [problem, text, again] of problems) {
      open({ problem });
      expect(target.querySelector(".problem")?.textContent, problem.kind).toContain(text);
      expect(button("retry") !== null, problem.kind).toBe(again);
      expect(listbox(), problem.kind).toBeNull();
    }
    button("retry")?.click();
    expect(retry).toHaveBeenCalledTimes(1);
    // the button goes with the problem: the keyboard is still in the dialog (Esc closes it)
    expect(dialog()?.contains(document.activeElement)).toBe(true);
  });

  it("a page that did not come leaves the rows, and Try again reads it instead of Show more", () => {
    open({ rows: [row(1), row(2)], next: 50, problem: { kind: "slow" } });
    expect(options()).toHaveLength(2);
    expect(target.querySelector(".problem")?.textContent).toContain("took too long");
    expect(button("retry")).not.toBeNull();
    expect(button("more")).toBeNull();
  });

  it("notes under the list: the name the file had before, a partial copy, how a pick went", () => {
    open({ rows: [row(1)] });
    expect(target.querySelector(".notes")).toBeNull();
    // the line a screen reader says is there before it has anything to say
    const said = target.querySelector(".note");
    expect(said?.getAttribute("role")).toBe("status");
    expect(said?.textContent).toBe("");
    open({
      rows: [row(1)],
      renamedFrom: "docs/manual.md",
      cut: true,
      note: "This version has the same text as the document now.",
    });
    expect(target.querySelector(".note")).toBe(said);
    expect(said?.textContent).toBe("This version has the same text as the document now.");
    const notes = target.querySelector(".notes");
    expect(notes?.textContent).toContain(
      "Earlier versions under the name docs/manual.md are not shown.",
    );
    expect(notes?.textContent).toContain(
      "Older versions may be missing: this is a partial copy of the repository.",
    );
    // after the rows
    const list = listbox();
    for (const under of [said, notes]) {
      expect(list && under && list.compareDocumentPosition(under)).toBe(
        Node.DOCUMENT_POSITION_FOLLOWING,
      );
    }
  });

  it("Esc and the close button close it, and the keyboard goes back to where it was", () => {
    const close = vi.spyOn(review, "closeVersions");
    open({ rows: [row(1)] });
    expect(dialog()?.contains(document.activeElement)).toBe(true);
    key(listbox(), "Escape");
    expect(close).toHaveBeenCalledTimes(1);
    expect(dialog()).toBeNull();
    expect(document.activeElement).toBe(opener);
    open({ rows: [row(1)] });
    expect(button("close")?.getAttribute("aria-label")).toBe("Close");
    button("close")?.click();
    flushSync();
    expect(close).toHaveBeenCalledTimes(2);
    expect(dialog()).toBeNull();
  });

  it("the keyboard is in the rows when they are there, and goes to them when they come", () => {
    open({ rows: [row(1)] });
    expect(document.activeElement).toBe(listbox());
    review.closeVersions();
    flushSync();
    // opened while reading: the dialog holds the keyboard until the rows come
    open({ loading: true });
    expect(document.activeElement).toBe(dialog());
    open({ rows: [row(1), row(2)] });
    expect(document.activeElement).toBe(listbox());
    // Tab never leaves the dialog
    key(listbox(), "Tab", { shiftKey: true });
    expect(dialog()?.contains(document.activeElement)).toBe(true);
  });

  it("a list asked for another document starts at its first row", () => {
    open({ rows: [row(1), row(2), row(3)] });
    key(listbox(), "End");
    expect(active()).toBe(2);
    shows("README.md");
    open({ path: "README.md", rows: [row(4), row(5), row(6)] });
    expect(active()).toBe(0);
  });
});
