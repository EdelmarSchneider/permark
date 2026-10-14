import { describe, expect, it } from "vitest";
import { FeedStore, mergeKind, visibleBulkItems, type FileEntry } from "./feed";
import type { FsBatch, FsChange } from "./types";

const batch = (changes: FsChange[], extra: Partial<FsBatch> = {}): FsBatch => ({
  changes,
  bulk: null,
  rescan: false,
  ...extra,
});
const ch = (path: string, atMs: number, kind: FsChange["kind"] = "modified"): FsChange => ({
  path,
  kind,
  atMs,
});
const file = (e: unknown): FileEntry => e as FileEntry;

describe("mergeKind", () => {
  it("keeps 'created' when a new file is saved again", () => {
    expect(mergeKind("created", "modified")).toBe("created");
  });
  it("treats delete + create as a modification", () => {
    expect(mergeKind("removed", "created")).toBe("modified");
  });
  it("lets removal and rename win", () => {
    expect(mergeKind("modified", "removed")).toBe("removed");
    expect(mergeKind("modified", "renamed")).toBe("renamed");
  });
});

describe("FeedStore", () => {
  it("keys entries by path and updates them in place, newest first", () => {
    const feed = new FeedStore();
    feed.apply(batch([ch("a.md", 1000)]), 1000);
    feed.apply(batch([ch("b.md", 2000)]), 2000);
    feed.apply(batch([ch("a.md", 3000)]), 3000);
    expect(feed.entries.map((e) => e.id)).toEqual(["a.md", "b.md"]);
    expect(file(feed.entries[0]).saves).toBe(2);
    expect(feed.entries[0].atMs).toBe(3000);
  });

  it("orders events of one batch by time", () => {
    const feed = new FeedStore();
    feed.apply(batch([ch("late.md", 3000), ch("early.md", 1000)]), 3000);
    expect(feed.entries.map((e) => e.id)).toEqual(["late.md", "early.md"]);
  });

  it("derives worktree label and folder", () => {
    const feed = new FeedStore();
    feed.apply(batch([ch("@wt/feature-ui/docs/ui.md", 1), ch("docs/specs/a.md", 2)]), 2);
    const [a, ui] = feed.entries.map(file);
    expect(a.worktree).toBeNull();
    expect(a.folder).toBe("docs/specs");
    expect(ui.worktree).toBe("feature-ui");
    expect(ui.folder).toBe("docs");
  });

  it("moves a renamed file to its new path and drops the old entry", () => {
    const feed = new FeedStore();
    feed.apply(batch([ch("docs/old.md", 1)]), 1);
    feed.apply(batch([{ kind: "renamed", path: "docs/new.md", from: "docs/old.md", atMs: 2 }]), 2);
    expect(feed.entries.map((e) => e.id)).toEqual(["docs/new.md"]);
    expect(file(feed.entries[0]).from).toBe("docs/old.md");
    expect(file(feed.entries[0]).kind).toBe("renamed");
  });

  it("tracks removal and re-creation of the same path", () => {
    const feed = new FeedStore();
    feed.apply(batch([ch("a.md", 1, "removed")]), 1);
    expect(file(feed.entries[0]).kind).toBe("removed");
    feed.apply(batch([ch("a.md", 2, "created")]), 2);
    expect(file(feed.entries[0]).kind).toBe("modified");
  });

  it("filters to docs only and counts the hidden files", () => {
    const feed = new FeedStore();
    feed.apply(batch([ch("a.md", 1), ch("b.json", 2), ch("c.png", 3)]), 3);
    const docs = feed.view(true);
    expect(docs.entries.map((e) => e.id)).toEqual(["a.md"]);
    expect(docs.hidden).toBe(2);
    expect(feed.view(false).entries).toHaveLength(3);
    expect(feed.view(false).hidden).toBe(0);
  });

  it("collapses a bulk batch into one entry and leaves the keyed entries alone", () => {
    const feed = new FeedStore();
    feed.apply(batch([ch("a.md", 1)]), 1);
    const update = feed.apply(
      batch([ch("x.md", 5), ch("y.rs", 6)], {
        bulk: { count: 143, reason: "gitOperation", paths: ["x.md", "y.rs"] },
      }),
      6,
    );
    expect(update.files).toEqual([]);
    expect(update.bulk).not.toBeNull();
    expect(feed.entries).toHaveLength(2);
    const bulk = feed.entries[0];
    expect(bulk.type).toBe("bulk");
    if (bulk.type === "bulk") {
      expect(bulk.count).toBe(143);
      expect(bulk.docCount).toBe(1);
      expect(bulk.atMs).toBe(6);
      expect(bulk.items.map((i) => i.path)).toEqual(["x.md", "y.rs"]);
    }
  });

  it("lists bulk.paths when the batch carries no changes, and keeps bulk rows in the docs-only view", () => {
    const feed = new FeedStore();
    feed.apply(
      batch([], {
        bulk: { count: 3, reason: "large", paths: ["a.md", "b.md", "c.txt"] },
        rescan: true,
      }),
      9000,
    );
    const bulk = feed.entries[0];
    expect(bulk.atMs).toBe(9000);
    if (bulk.type === "bulk") {
      expect(bulk.rescan).toBe(true);
      expect(bulk.items.map((i) => i.kind)).toEqual(["modified", "modified", "modified"]);
    }
    expect(feed.view(true).entries).toHaveLength(1);
  });

  it("caps the bulk item list shown until 'Show all'", () => {
    const paths = Array.from({ length: 80 }, (_, i) => `f${i}.md`);
    const feed = new FeedStore();
    feed.apply(batch([], { bulk: { count: 80, reason: "gitOperation", paths } }), 1);
    const bulk = feed.entries[0];
    if (bulk.type !== "bulk") throw new Error("expected a bulk entry");
    expect(visibleBulkItems(bulk, false)).toHaveLength(50);
    expect(visibleBulkItems(bulk, true)).toHaveLength(80);
  });

  it("returns the entries a batch touched so callers can toast and follow", () => {
    const feed = new FeedStore();
    const update = feed.apply(batch([ch("a.md", 1), ch("b.md", 2), ch("a.md", 3)]), 3);
    // one entry per path, in the order they were last touched
    expect(update.files.map((f) => f.path)).toEqual(["b.md", "a.md"]);
    expect(update.files.map((f) => f.saves)).toEqual([1, 2]);
  });

  it("reports the time of the newest event", () => {
    const feed = new FeedStore();
    expect(feed.lastEventMs).toBeNull();
    feed.apply(batch([ch("a.md", 4200)]), 5000);
    expect(feed.lastEventMs).toBe(4200);
  });

  it("shows an operation of the Explorer as one row on top, whatever the filter", () => {
    const feed = new FeedStore();
    feed.apply(batch([ch("a.md", 1000)]), 1000);
    const op = feed.addOperation({
      kind: "moved",
      atMs: 2000,
      summary: "Moved 2 files, 7 links updated",
      items: [
        { path: "manual/a.md", from: "docs/a.md" },
        { path: "manual/b.md", from: "docs/b.md" },
      ],
      notes: [],
    });
    expect(op.type).toBe("op");
    expect(feed.entries.map((e) => e.id)).toEqual([op.id, "a.md"]);
    expect(feed.lastEventMs).toBe(2000);
    // "Docs only" hides files that are not documents, never an operation
    feed.apply(batch([ch("data.json", 3000)]), 3000);
    const docsOnly = feed.view(true);
    expect(docsOnly.entries.map((e) => e.id)).toEqual([op.id, "a.md"]);
    expect(docsOnly.hidden).toBe(1);
  });

  it("gives every operation its own id", () => {
    const feed = new FeedStore();
    const op = {
      kind: "deleted" as const,
      atMs: 1,
      summary: "Deleted 1 file",
      items: [],
      notes: [],
    };
    const first = feed.addOperation(op);
    const second = feed.addOperation(op);
    expect(first.id).not.toBe(second.id);
    expect(feed.entries).toHaveLength(2);
  });
});
