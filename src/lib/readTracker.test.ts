import { beforeEach, describe, expect, it } from "vitest";
import { READ_AFTER_MS, ReadTracker, type Scheduler } from "./readTracker";

/** A clock that only moves when the test says so. */
class FakeClock implements Scheduler {
  now = 0;
  private tasks: { at: number; run: () => void; id: number }[] = [];
  private seq = 0;

  after(ms: number, run: () => void): () => void {
    const id = ++this.seq;
    this.tasks.push({ at: this.now + ms, run, id });
    return () => {
      this.tasks = this.tasks.filter((task) => task.id !== id);
    };
  }

  get pending(): number {
    return this.tasks.length;
  }

  advance(ms: number): void {
    const until = this.now + ms;
    for (;;) {
      const due = this.tasks.filter((task) => task.at <= until).sort((a, b) => a.at - b.at)[0];
      if (!due) break;
      this.tasks = this.tasks.filter((task) => task !== due);
      this.now = due.at;
      due.run();
    }
    this.now = until;
  }
}

let clock: FakeClock;
let unread: Set<string>;
let marked: string[];
let tracker: ReadTracker;

beforeEach(() => {
  clock = new FakeClock();
  unread = new Set(["a.md", "b.md"]);
  marked = [];
  tracker = new ReadTracker({
    scheduler: clock,
    markRead: (path) => {
      marked.push(path);
      unread.delete(path);
    },
    isUnread: (path) => unread.has(path),
  });
});

/** A focused window with a.md on show. */
function reading(path = "a.md"): void {
  tracker.setFocused(true);
  tracker.show(path, true);
}

describe("a document that stays on show", () => {
  it("is read after three seconds, not before", () => {
    reading();
    clock.advance(READ_AFTER_MS - 1);
    expect(marked).toEqual([]);
    clock.advance(1);
    expect(marked).toEqual(["a.md"]);
  });

  it("is told to the backend once", () => {
    reading();
    clock.advance(READ_AFTER_MS * 5);
    expect(marked).toEqual(["a.md"]);
    expect(clock.pending).toBe(0);
  });

  it("says nothing when the document was read already", () => {
    unread.delete("a.md");
    reading();
    clock.advance(READ_AFTER_MS * 2);
    expect(marked).toEqual([]);
  });

  it("is not read while the window has no focus, and the three seconds start when it gets one", () => {
    tracker.show("a.md", true);
    clock.advance(READ_AFTER_MS * 2);
    expect(marked).toEqual([]);
    tracker.setFocused(true);
    clock.advance(READ_AFTER_MS - 1);
    expect(marked).toEqual([]);
    clock.advance(1);
    expect(marked).toEqual(["a.md"]);
  });

  it("starts the three seconds again after the window lost the focus for a while", () => {
    reading();
    clock.advance(2000);
    tracker.setFocused(false);
    clock.advance(60_000);
    tracker.setFocused(true);
    clock.advance(2999);
    expect(marked).toEqual([]);
    clock.advance(1);
    expect(marked).toEqual(["a.md"]);
  });

  it("is not read in the editor, and is read from the start when the reading view comes back", () => {
    reading();
    clock.advance(2000);
    tracker.show("a.md", false);
    clock.advance(60_000);
    expect(marked).toEqual([]);
    tracker.show("a.md", true);
    clock.advance(2999);
    expect(marked).toEqual([]);
    clock.advance(1);
    expect(marked).toEqual(["a.md"]);
  });

  it("restarts the wait for the next document, and stops waiting when there is none", () => {
    reading("a.md");
    clock.advance(2000);
    tracker.show("b.md", true);
    clock.advance(2999);
    expect(marked).toEqual([]);
    clock.advance(1);
    expect(marked).toEqual(["b.md"]);

    tracker.show(null, false);
    expect(clock.pending).toBe(0);
  });
});

describe("a document that is scrolled", () => {
  it("is read at once", () => {
    reading();
    tracker.scrolled();
    expect(marked).toEqual(["a.md"]);
    expect(clock.pending).toBe(0);
  });

  it("is not read by a scroll in a window without focus, or in the editor, or with nothing on show", () => {
    tracker.show("a.md", true);
    tracker.scrolled();
    tracker.setFocused(true);
    tracker.show("a.md", false);
    tracker.scrolled();
    tracker.show(null, true);
    tracker.scrolled();
    expect(marked).toEqual([]);
  });

  it("counts for the document that is on show, not for the one before", () => {
    reading("a.md");
    tracker.scrolled();
    tracker.show("b.md", true);
    expect(marked).toEqual(["a.md"]);
    clock.advance(READ_AFTER_MS);
    expect(marked).toEqual(["a.md", "b.md"]);
  });
});

describe("a change of the file on show", () => {
  it("stays read when it arrives in a focused window after the document was read", () => {
    reading();
    tracker.scrolled();
    expect(marked).toEqual(["a.md"]);
    // the agent saves: the backend sees a different file, and the person is watching
    unread.add("a.md");
    tracker.contentChanged("a.md");
    expect(marked).toEqual(["a.md", "a.md"]);
    expect(unread.has("a.md")).toBe(false);
  });

  it("tells the backend even when it has not noticed the change yet", () => {
    reading();
    tracker.scrolled();
    tracker.contentChanged("a.md");
    expect(marked).toEqual(["a.md", "a.md"]);
  });

  it("does not hurry a document that is still being read for the first time", () => {
    reading();
    clock.advance(1000);
    tracker.contentChanged("a.md");
    expect(marked).toEqual([]);
    clock.advance(2000);
    expect(marked).toEqual(["a.md"]);
  });

  it("makes the document unread until the rule passes again when nobody watched", () => {
    reading();
    tracker.scrolled();
    tracker.setFocused(false);
    unread.add("a.md");
    tracker.contentChanged("a.md");
    clock.advance(60_000);
    expect(marked).toEqual(["a.md"]);
    // back in the window: three seconds, or a scroll
    tracker.setFocused(true);
    clock.advance(2999);
    expect(marked).toEqual(["a.md"]);
    clock.advance(1);
    expect(marked).toEqual(["a.md", "a.md"]);
  });

  it("is the same for a document in the editor: the person has not seen it", () => {
    reading();
    tracker.scrolled();
    tracker.show("a.md", false);
    unread.add("a.md");
    tracker.contentChanged("a.md");
    expect(marked).toEqual(["a.md"]);
    tracker.show("a.md", true);
    tracker.scrolled();
    expect(marked).toEqual(["a.md", "a.md"]);
  });

  it("ignores the changes of other files", () => {
    reading();
    tracker.scrolled();
    tracker.contentChanged("b.md");
    expect(marked).toEqual(["a.md"]);
  });
});

describe("the unread documents change behind the person's back", () => {
  it("reads the document on show if it was due and became unread (the first look at the folder came late)", () => {
    unread.delete("a.md");
    reading();
    tracker.scrolled();
    expect(marked).toEqual([]);
    unread.add("a.md");
    tracker.unreadChanged();
    expect(marked).toEqual(["a.md"]);
  });

  it("does nothing for a document that is not due yet", () => {
    unread.delete("a.md");
    reading();
    unread.add("a.md");
    tracker.unreadChanged();
    expect(marked).toEqual([]);
    clock.advance(READ_AFTER_MS);
    expect(marked).toEqual(["a.md"]);
  });
});

describe("a document the person marked as unread", () => {
  it("is not read again while it stays on show", () => {
    reading();
    tracker.scrolled();
    unread.add("a.md");
    tracker.hold();
    tracker.unreadChanged();
    tracker.scrolled();
    tracker.contentChanged("a.md");
    clock.advance(READ_AFTER_MS * 3);
    expect(marked).toEqual(["a.md"]);
  });

  it("stops the wait that was running", () => {
    reading();
    clock.advance(1000);
    tracker.hold();
    clock.advance(READ_AFTER_MS * 3);
    expect(marked).toEqual([]);
    expect(clock.pending).toBe(0);
  });

  it("can be read once the person comes back to it", () => {
    reading("a.md");
    tracker.hold();
    tracker.show("b.md", true);
    tracker.show("a.md", true);
    clock.advance(READ_AFTER_MS);
    expect(marked).toEqual(["a.md"]);
  });

  it("stays held through a change of focus", () => {
    reading();
    tracker.hold();
    tracker.setFocused(false);
    tracker.setFocused(true);
    clock.advance(READ_AFTER_MS * 3);
    expect(marked).toEqual([]);
  });
});
