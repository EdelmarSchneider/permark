// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReviewView } from "../../api";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { documents } = await import("../../core/doc/document.svelte");
const { language } = await import("../../core/i18n");
const { exactTime } = await import("../../core/time");
const { review } = await import("./store.svelte");
const { default: ReviewBar } = await import("./ReviewBar.svelte");

const VIEW: ReviewView = {
  path: "docs/guide.md",
  hash: "ab".repeat(8),
  basis: "read",
  asked: "read",
  unit: "blocks",
  count: 2,
  canKeep: true,
  uncommitted: false,
  changes: [],
};

const COMMIT = "7c9e21f0a4b83d5566e1f2a39b0c4d7e8f1a2b3c";
const AT = Date.UTC(2026, 10, 13, 16, 57);
/** The same review against an earlier version; the file is what the last commit holds. */
const OLDER: ReviewView = {
  ...VIEW,
  basis: { version: COMMIT },
  asked: { version: COMMIT },
  canKeep: false,
};
const NO_REPOSITORY = "This folder is not a git repository.";

let target: HTMLElement;
let tree: HTMLElement;
let component: Record<string, unknown> | null = null;

beforeEach(() => {
  document.body.replaceChildren();
  tree = document.createElement("div");
  tree.setAttribute("role", "tree");
  tree.tabIndex = 0;
  const area = document.createElement("section");
  area.className = "area";
  target = document.createElement("div");
  const scroller = document.createElement("div");
  scroller.dataset["scroller"] = "";
  scroller.tabIndex = -1;
  area.append(target, scroller);
  document.body.append(tree, area);
  review.state = { status: "idle" };
  review.basis = "read";
  component = mount(ReviewBar, { target });
  flushSync();
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  review.cancelBringBack();
  review.state = { status: "idle" };
  review.basisNote = null;
  vi.restoreAllMocks();
});

const radios = (): HTMLElement[] =>
  Array.from(target.querySelectorAll<HTMLElement>('[role="radio"]'));

/** The bar's button that reads `label`. */
const button = (label: string): HTMLButtonElement | undefined =>
  Array.from(target.querySelectorAll<HTMLButtonElement>("button")).find(
    (b) => (b.textContent ?? "").trim() === label,
  );

const question = (): HTMLElement | null =>
  target.querySelector<HTMLElement>('[role="alertdialog"]');

/** Whether git can be asked here (`why`: the sentence that says it cannot). Before the review is drawn. */
function git(why: string | null): void {
  vi.spyOn(review, "commitUnavailable", "get").mockReturnValue(why);
}

/** The review on screen, against what was read. */
function reviewing(): void {
  review.state = { status: "reviewing", view: VIEW, at: 0 };
  flushSync();
}

/** The review on screen compares with an earlier version, as after a pick in the list. */
function againstVersion(view: ReviewView = OLDER): void {
  vi.spyOn(review, "picked", "get").mockReturnValue({ commit: COMMIT, atMs: AT });
  // the review belongs to the document shown: its buttons act
  vi.spyOn(documents, "path", "get").mockReturnValue(view.path);
  review.basis = { version: COMMIT };
  review.state = { status: "reviewing", view, at: 0 };
  flushSync();
}

/** The backend puts everything back when asked; returns the calls that asked for it. */
function backend(): () => number {
  const sent = vi
    .spyOn(api, "pluginCall")
    .mockResolvedValue({ ok: true, value: { ...OLDER, count: 0 } });
  return () => sent.mock.calls.filter(([method]) => method === "review.revertAll").length;
}

function key(el: HTMLElement, name: string, init: KeyboardEventInit = {}): void {
  el.dispatchEvent(
    new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true, ...init }),
  );
  flushSync();
}

describe("the review bar", () => {
  it("the offer floats over the top of the page: the page under it never moves when it appears", () => {
    review.state = { status: "offer", path: "docs/guide.md", count: 3 };
    flushSync();
    const offer = target.querySelector<HTMLElement>(".bar");
    expect(offer?.textContent).toContain("Changed since you last read");
    // the strip sits in a zero-height anchor out of the flow (an overlay), not in the column above the page
    const anchor = offer?.parentElement;
    expect(anchor?.classList.contains("overlay")).toBe(true);
    expect(offer?.classList.contains("floating")).toBe(true);
  });

  it("opened from the palette (the keyboard went back to the file list): the keyboard goes to the bar", () => {
    tree.focus();
    review.state = { status: "reviewing", view: VIEW, at: 0 };
    flushSync();
    expect(target.querySelector(".bar")?.contains(document.activeElement)).toBe(true);
  });

  it("Compare with = one Tab stop; arrows move the choice and the keyboard with it", async () => {
    review.state = { status: "reviewing", view: VIEW, at: 0 };
    flushSync();
    expect(radios().map((r) => r.tabIndex)).toEqual([0, -1, -1]);
    const set = vi.spyOn(review, "setBasis").mockResolvedValue();
    radios()[0]!.focus();
    key(radios()[0]!, "ArrowRight");
    await tick();
    expect(set).toHaveBeenCalledWith("commit");
    expect(document.activeElement).toBe(radios()[1]);
    // a modified arrow is not a move (Alt+Left = back)
    key(radios()[1]!, "ArrowLeft", { altKey: true });
    expect(set).toHaveBeenCalledTimes(1);
    key(radios()[1]!, "ArrowLeft");
    expect(set).toHaveBeenLastCalledWith("read");
  });
});

describe("an earlier version in the review bar", () => {
  it("is the third thing to compare with: until one is picked it only offers the list", () => {
    git(null);
    reviewing();
    expect(radios()).toHaveLength(3);
    const version = radios()[2]!;
    expect((version.textContent ?? "").trim()).toBe("an earlier version…");
    expect(radios().map((r) => r.getAttribute("aria-checked"))).toEqual(["true", "false", "false"]);
    expect(radios().map((r) => r.tabIndex)).toEqual([0, -1, -1]);
    expect(version.getAttribute("aria-disabled")).toBe("false");
    expect(version.classList.contains("is-off")).toBe(false);
    expect(button("Revert all")).toBeDefined();
    expect(button("Bring back this version")).toBeUndefined();
  });

  it("picked: the choice names the version, is the one checked and the one Tab stop", () => {
    git(null);
    againstVersion();
    const when = exactTime(AT, language.locale);
    const version = radios()[2]!;
    expect((version.textContent ?? "").trim()).toBe(`the version of ${when}`);
    // the label may be cut where the bar is narrow: the whole sentence is in the title
    expect(version.title).toBe(`Comparing with the version of ${when}`);
    expect(radios().map((r) => r.getAttribute("aria-checked"))).toEqual(["false", "false", "true"]);
    expect(radios().map((r) => r.tabIndex)).toEqual([-1, -1, 0]);
    expect(version.classList.contains("is-on")).toBe(true);
    expect(button("Bring back this version")).toBeDefined();
    expect(button("Revert all")).toBeUndefined();
  });

  it("a click opens the list of versions and chooses nothing; Enter and Space are the button's click", () => {
    git(null);
    const open = vi.spyOn(review, "openVersions").mockResolvedValue();
    const set = vi.spyOn(review, "setBasis").mockResolvedValue();
    reviewing();
    const version = radios()[2]!;
    // a native button: the web view turns Enter and Space on it into this click
    expect(version.tagName).toBe("BUTTON");
    expect(version.getAttribute("type")).toBe("button");
    version.click();
    expect(open).toHaveBeenCalledTimes(1);
    expect(set).not.toHaveBeenCalled();
  });

  it("a version is picked already: a click opens the list again, to pick another", () => {
    git(null);
    const open = vi.spyOn(review, "openVersions").mockResolvedValue();
    const set = vi.spyOn(review, "setBasis").mockResolvedValue();
    againstVersion();
    radios()[2]!.click();
    expect(open).toHaveBeenCalledTimes(1);
    expect(set).not.toHaveBeenCalled();
  });

  it("an arrow onto it only brings the keyboard; an arrow from it to what was read chooses that", async () => {
    git(null);
    const open = vi.spyOn(review, "openVersions").mockResolvedValue();
    const set = vi.spyOn(review, "setBasis").mockResolvedValue();
    reviewing();
    radios()[1]!.focus();
    key(radios()[1]!, "ArrowRight");
    await tick();
    expect(document.activeElement).toBe(radios()[2]);
    expect(open).not.toHaveBeenCalled();
    expect(set).not.toHaveBeenCalled();
    key(radios()[2]!, "ArrowRight");
    await tick();
    expect(set).toHaveBeenCalledTimes(1);
    expect(set).toHaveBeenLastCalledWith("read");
    expect(document.activeElement).toBe(radios()[0]);
    // backwards from the first one, around the group
    key(radios()[0]!, "ArrowLeft");
    await tick();
    expect(document.activeElement).toBe(radios()[2]);
    expect(open).not.toHaveBeenCalled();
    expect(set).toHaveBeenCalledTimes(1);
    key(radios()[2]!, "ArrowLeft");
    await tick();
    expect(set).toHaveBeenLastCalledWith("commit");
  });

  it("git cannot be asked: the choice says why, and choosing it opens nothing", () => {
    git(NO_REPOSITORY);
    const open = vi.spyOn(review, "openVersions").mockResolvedValue();
    reviewing();
    const version = radios()[2]!;
    expect(version.getAttribute("aria-disabled")).toBe("true");
    expect(version.title).toBe(NO_REPOSITORY);
    expect(version.classList.contains("is-off")).toBe(true);
    expect(target.querySelector('[role="status"]')).toBeNull();
    version.click();
    flushSync();
    expect(open).not.toHaveBeenCalled();
    expect(target.querySelector('[role="status"]')?.textContent).toBe(NO_REPOSITORY);
  });
});

describe("bringing back an earlier version from the bar", () => {
  /** Presses "Bring back this version" with the keyboard on it. */
  function ask(): void {
    const bringBack = button("Bring back this version");
    bringBack?.focus();
    bringBack?.click();
    flushSync();
  }

  it("text that is in no commit would be replaced: the bar asks first, and only the yes writes", async () => {
    const reverts = backend();
    againstVersion({ ...OLDER, uncommitted: true });
    expect(question()).toBeNull();
    ask();
    expect(question()?.getAttribute("aria-label")).toBe("Bring back this version");
    expect(question()?.textContent).toContain(
      "Your changes since the last commit are not saved in git.",
    );
    // the question names the version, and promises only the undo that is offered right after
    const said = question()?.getAttribute("aria-describedby") ?? "";
    expect(document.getElementById(said)?.textContent).toBe(
      `Your changes since the last commit are not saved in git. Bringing back the version of ${exactTime(AT, language.locale)} replaces them. You can undo it right afterwards.`,
    );
    // the question takes the place of the bar's actions; what is compared with stays
    expect(button("Bring back this version")).toBeUndefined();
    expect(button("Done")).toBeUndefined();
    expect(radios()).toHaveLength(3);
    // the keyboard waits on the answer that changes nothing
    expect(document.activeElement).toBe(button("Cancel"));
    expect(reverts()).toBe(0);
    button("Bring back")?.click();
    await vi.waitFor(() => expect(review.state.status).toBe("idle"));
    expect(reverts()).toBe(1);
  });

  it("Cancel: nothing is written and the keyboard is back on the button that asked", () => {
    const reverts = backend();
    againstVersion({ ...OLDER, uncommitted: true });
    ask();
    button("Cancel")?.click();
    flushSync();
    expect(question()).toBeNull();
    expect(reverts()).toBe(0);
    expect(review.state.status).toBe("reviewing");
    expect(document.activeElement).toBe(button("Bring back this version"));
  });

  it("Esc is Cancel, and goes no further: the review stays open", () => {
    const reverts = backend();
    againstVersion({ ...OLDER, uncommitted: true });
    ask();
    const esc = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    button("Cancel")?.dispatchEvent(esc);
    flushSync();
    expect(esc.defaultPrevented).toBe(true);
    expect(question()).toBeNull();
    expect(reverts()).toBe(0);
    expect(review.state.status).toBe("reviewing");
    expect(document.activeElement).toBe(button("Bring back this version"));
  });

  it("the file is what the last commit holds: the version is brought back at once", async () => {
    const reverts = backend();
    againstVersion();
    ask();
    expect(question()).toBeNull();
    await vi.waitFor(() => expect(review.state.status).toBe("idle"));
    expect(reverts()).toBe(1);
  });
});
