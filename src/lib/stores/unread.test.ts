import { beforeEach, describe, expect, it, vi } from "vitest";

// The backend is replaced: these tests are about what the store knows, what it tells the backend, and when.
const fake = vi.hoisted(() => ({
  getUnread: vi.fn(),
  markRead: vi.fn(),
  markUnread: vi.fn(),
  markAllRead: vi.fn(),
  markFolderRead: vi.fn(),
}));
vi.mock("../api", () => ({ api: { ...fake, on: async () => () => {} } }));

import { READ_AFTER_MS, type Scheduler } from "../readTracker";
import { toast } from "./toast.svelte";
import { UnreadStore } from "./unread.svelte";

/** A clock that only moves when the test says so. */
class Clock implements Scheduler {
  private tasks: { at: number; run: () => void }[] = [];
  now = 0;
  after(ms: number, run: () => void): () => void {
    const task = { at: this.now + ms, run };
    this.tasks.push(task);
    return () => {
      this.tasks = this.tasks.filter((t) => t !== task);
    };
  }
  advance(ms: number): void {
    this.now += ms;
    for (const task of this.tasks.filter((t) => t.at <= this.now)) {
      this.tasks = this.tasks.filter((t) => t !== task);
      task.run();
    }
  }
}

let clock: Clock;
let store: UnreadStore;

beforeEach(() => {
  for (const fn of Object.values(fake)) fn.mockReset().mockResolvedValue(undefined);
  toast.clear();
  clock = new Clock();
  store = new UnreadStore(clock);
});

describe("the unread documents", () => {
  it("are what the backend last said, with the number inside each folder", () => {
    store.onChanged(["docs/specs/a.md", "docs/b.md", "README.md"]);
    expect(store.count).toBe(3);
    expect(store.isUnread("docs/b.md")).toBe(true);
    expect(store.isUnread("docs/c.md")).toBe(false);
    expect(store.inFolder("docs")).toBe(2);
    expect(store.inFolder("docs/specs")).toBe(1);
    expect(store.inFolder("other")).toBe(0);
    store.onChanged([]);
    expect(store.count).toBe(0);
    expect(store.inFolder("docs")).toBe(0);
  });

  it("are asked for when a project opens", async () => {
    fake.getUnread.mockResolvedValue(["a.md"]);
    await store.refresh();
    expect(store.paths).toEqual(["a.md"]);
  });

  it("are not overwritten by an answer that a newer event has overtaken", async () => {
    let answer: (paths: string[]) => void = () => {};
    fake.getUnread.mockReturnValue(new Promise<string[]>((resolve) => (answer = resolve)));
    const pending = store.refresh();
    store.onChanged(["new.md"]);
    answer(["old.md"]);
    await pending;
    expect(store.paths).toEqual(["new.md"]);
  });

  it("keep what they were when the question fails", async () => {
    store.onChanged(["a.md"]);
    fake.getUnread.mockRejectedValue(new Error("no"));
    await store.refresh();
    expect(store.paths).toEqual(["a.md"]);
  });

  it("are forgotten, with the Explorer's filter, when the project changes", () => {
    store.onChanged(["a.md"]);
    store.toggleOnlyUnread();
    store.reset();
    expect(store.count).toBe(0);
    expect(store.onlyUnread).toBe(false);
  });
});

describe("reading", () => {
  it("tells the backend when an unread document has been on show long enough", () => {
    store.onChanged(["a.md"]);
    store.setFocused(true);
    store.watch("a.md", true);
    clock.advance(READ_AFTER_MS - 1);
    expect(fake.markRead).not.toHaveBeenCalled();
    clock.advance(1);
    expect(fake.markRead).toHaveBeenCalledExactlyOnceWith("a.md");
  });

  it("tells it when the person scrolls", () => {
    store.onChanged(["a.md"]);
    store.setFocused(true);
    store.watch("a.md", true);
    store.scrolled();
    expect(fake.markRead).toHaveBeenCalledExactlyOnceWith("a.md");
  });

  it("says nothing about a document that is read already", () => {
    store.setFocused(true);
    store.watch("a.md", true);
    clock.advance(READ_AFTER_MS * 2);
    expect(fake.markRead).not.toHaveBeenCalled();
  });

  it("keeps a document the person marked as unread unread while it stays on show", async () => {
    store.setFocused(true);
    store.watch("a.md", true);
    store.scrolled();
    await store.markUnread("a.md", true);
    expect(fake.markUnread).toHaveBeenCalledExactlyOnceWith("a.md");
    store.onChanged(["a.md"]);
    clock.advance(READ_AFTER_MS * 3);
    expect(fake.markRead).not.toHaveBeenCalled();
  });

  it("does not hold a document that is not on show", async () => {
    store.setFocused(true);
    store.watch("a.md", true);
    await store.markUnread("b.md", false);
    store.onChanged(["a.md"]);
    store.scrolled();
    expect(fake.markRead).toHaveBeenCalledExactlyOnceWith("a.md");
  });

  it("does not trouble the person when the quiet marking fails", async () => {
    fake.markRead.mockRejectedValue(new Error("gone"));
    await store.markRead("a.md");
    expect(toast.current).toBeNull();
  });
});

describe("what the person asks for", () => {
  it("goes to the backend", async () => {
    await store.markAllRead();
    await store.markFolderRead("docs");
    expect(fake.markAllRead).toHaveBeenCalledOnce();
    expect(fake.markFolderRead).toHaveBeenCalledExactlyOnceWith("docs");
  });

  it("says so when it fails", async () => {
    fake.markAllRead.mockRejectedValue({ kind: "internal", message: "no", detail: "x" });
    await store.markAllRead();
    expect(toast.current).toMatchObject({ type: "message", tone: "warn" });
  });

  it("switches the Explorer between everything and only what is unread", () => {
    expect(store.onlyUnread).toBe(false);
    store.toggleOnlyUnread();
    expect(store.onlyUnread).toBe(true);
    store.toggleOnlyUnread();
    expect(store.onlyUnread).toBe(false);
  });
});
