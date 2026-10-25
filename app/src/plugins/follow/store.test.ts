// @vitest-environment jsdom
import "../../core/testing/media";
import { afterEach, describe, expect, it, vi } from "vitest";
import { documents } from "../../core/doc/document.svelte";
import { PluginClient } from "../../core/plugins/client";
import { createMockApi } from "../../api/mock";
import { entry } from "../live-feed/testing";
import { FollowStore, followTarget } from "./store.svelte";

afterEach(() => vi.restoreAllMocks());

describe("what to follow", () => {
  it("is the newest document created, changed or moved by someone else", () => {
    expect(followTarget([entry(1, "a.md"), entry(2, "b.md")])).toBe("b.md");
    expect(followTarget([entry(1, "a.md", "created")])).toBe("a.md");
    expect(followTarget([entry(1, "a.md", "renamed")])).toBe("a.md");
  });

  it("ignores the person's own doing, removals, folders, files that are not documents and groups", () => {
    expect(
      followTarget([
        entry(1, "a.md", "modified", { own: true }),
        entry(2, "b.md", "removed"),
        entry(3, "c.md", "modified", { isDir: true }),
        entry(4, "d.json"),
        { item: "group", id: 5, reason: "git", count: 9, atMs: 1, sample: [], own: false },
        { item: "resync", id: 6, atMs: 2 },
      ]),
    ).toBeNull();
  });
});

describe("what to follow, silenced", () => {
  it("skips what the person silenced", () => {
    const silenced = (path: string): boolean => path.startsWith("quiet/");
    expect(followTarget([entry(1, "loud/a.md"), entry(2, "quiet/b.md")], silenced)).toBe(
      "loud/a.md",
    );
    expect(followTarget([entry(2, "quiet/b.md")], silenced)).toBeNull();
  });
});

describe("follow mode", () => {
  function setup(busy: boolean | (() => boolean) = false) {
    const navigate = vi.spyOn(documents, "navigate").mockImplementation(() => {});
    const store = new FollowStore(
      new PluginClient(createMockApi(), () => "root"),
      () => "root",
      typeof busy === "function" ? busy : () => busy,
    );
    return { store, navigate };
  }

  it("does nothing while it is off", () => {
    const { store, navigate } = setup();
    store.consider([entry(1, "a.md")]);
    expect(navigate).not.toHaveBeenCalled();
  });

  it("opens the document being written and remembers where the person was", () => {
    vi.spyOn(documents, "path", "get").mockReturnValue("here.md");
    const { store, navigate } = setup();
    store.toggle();
    expect(store.on).toBe(true);
    store.consider([entry(1, "a.md")]);
    expect(navigate).toHaveBeenCalledWith("a.md", null);
    expect(store.jumpedFrom).toBe("here.md");
    // a second jump keeps the first place
    vi.spyOn(documents, "path", "get").mockReturnValue("a.md");
    store.consider([entry(2, "b.md")]);
    expect(store.jumpedFrom).toBe("here.md");
    // back
    store.goBack();
    expect(navigate).toHaveBeenLastCalledWith("here.md", null);
    expect(store.jumpedFrom).toBeNull();
  });

  it("stays where it is for the document already open and while the person edits", () => {
    vi.spyOn(documents, "path", "get").mockReturnValue("a.md");
    const open = setup();
    open.store.toggle();
    open.store.consider([entry(1, "a.md")]);
    expect(open.navigate).not.toHaveBeenCalled();
    const busy = setup(true);
    busy.store.toggle();
    busy.store.consider([entry(2, "b.md")]);
    expect(busy.navigate).not.toHaveBeenCalled();
  });

  it("stopping forgets the way back, and toggling twice turns it off", () => {
    vi.spyOn(documents, "path", "get").mockReturnValue("here.md");
    const { store } = setup();
    store.toggle();
    store.consider([entry(1, "a.md")]);
    expect(store.jumpedFrom).toBe("here.md");
    store.toggle();
    expect(store.on).toBe(false);
    expect(store.jumpedFrom).toBeNull();
  });

  it("is paused while the person edits and follows again after, with no catch-up jump", () => {
    vi.spyOn(documents, "path", "get").mockReturnValue("here.md");
    let editing = false;
    const { store, navigate } = setup(() => editing);
    expect(store.paused).toBe(false);
    store.toggle();
    expect(store.paused).toBe(false);
    editing = true;
    expect(store.paused).toBe(true);
    store.consider([entry(1, "a.md")]);
    expect(navigate).not.toHaveBeenCalled();
    editing = false;
    expect(store.paused).toBe(false);
    // nothing missed is replayed; the next change is followed
    expect(navigate).not.toHaveBeenCalled();
    store.consider([entry(2, "b.md")]);
    expect(navigate).toHaveBeenCalledWith("b.md", null);
    store.stop();
    editing = true;
    expect(store.paused).toBe(false);
  });

  it("the toast's Follow turns it on and opens the latest as the first jump", () => {
    vi.spyOn(documents, "path", "get").mockReturnValue("here.md");
    const { store, navigate } = setup();
    store.followTo("plan.md");
    expect(store.on).toBe(true);
    expect(navigate).toHaveBeenCalledWith("plan.md", null);
    expect(store.jumpedFrom).toBe("here.md");
    // while editing: on, but no jump
    const busy = setup(true);
    busy.navigate.mockClear();
    busy.store.followTo("plan.md");
    expect(busy.store.on).toBe(true);
    expect(busy.navigate).not.toHaveBeenCalled();
  });
});
