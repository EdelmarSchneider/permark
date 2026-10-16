import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DocumentView } from "../types";

// The backend is replaced: these tests are about how a jump along the trail moves the history and what it opens.
const fake = vi.hoisted(() => ({
  readDocument: vi.fn(),
  fileMeta: vi.fn(),
}));
vi.mock("../api", () => ({
  api: {
    ...fake,
    metaIndex: async () => [],
    listTags: async () => ({ tags: [], untagged: 0, missing: 0, status: {} }),
    on: async () => () => {},
  },
}));
vi.mock("../platform", () => ({
  setWindowTitle: vi.fn(async () => {}),
  askUnsaved: vi.fn(async () => "keep"),
  askYesNo: vi.fn(async () => true),
  copyText: vi.fn(async () => true),
  guardWindowClose: vi.fn(async () => () => {}),
  checkForUpdate: vi.fn(),
  assetUrl: (p: string) => p,
  isMock: true,
}));

import { docs } from "./docs.svelte";
import { toast } from "./toast.svelte";

const view = (path: string): DocumentView => ({
  path,
  kind: "markdown",
  html: "",
  blocks: [],
  title: null,
  frontmatter: null,
  modifiedMs: 1,
  sizeBytes: 1,
  encodingNote: null,
});

/** Opens a, b, c, d in that order: the reader ends on d. */
async function visitAll(): Promise<void> {
  for (const path of ["a.md", "b.md", "c.md", "d.md"]) await docs.open(path);
}

beforeEach(() => {
  docs.reset();
  docs.guard = null;
  toast.clear();
  for (const mock of Object.values(fake)) mock.mockReset();
  fake.readDocument.mockImplementation(async (path: string) => view(path));
  fake.fileMeta.mockResolvedValue({ path: "", tags: [], description: "", writable: true });
});

describe("jumping along the trail", () => {
  it("opens the document several steps back and keeps the others to come back to", async () => {
    await visitAll();
    await docs.go(-3);
    expect(docs.path).toBe("a.md");
    expect(docs.canBack).toBe(false);
    expect(docs.canForward).toBe(true);
    expect(docs.trailStops("forward").map((s) => s.path)).toEqual(["b.md", "c.md", "d.md"]);
    await docs.forward();
    expect(docs.path).toBe("b.md");
  });

  it("opens a document ahead after going back, and goes on from there", async () => {
    await visitAll();
    await docs.go(-3);
    await docs.go(2);
    expect(docs.path).toBe("c.md");
    expect(docs.canBack).toBe(true);
    expect(docs.canForward).toBe(true);
    await docs.back();
    expect(docs.path).toBe("b.md");
  });

  it("adds no entry for the document it opens, so a new visit still drops what was ahead", async () => {
    await visitAll();
    await docs.go(-2);
    expect(docs.trail().map((s) => s.path)).toEqual(["d.md", "c.md", "b.md", "a.md"]);
    await docs.open("e.md");
    expect(docs.trail().map((s) => s.path)).toEqual(["e.md", "b.md", "a.md"]);
  });

  it("marks the open document in the trail", async () => {
    await visitAll();
    await docs.go(-1);
    expect(docs.trail().find((s) => s.current)?.path).toBe("c.md");
    expect(docs.trail().filter((s) => s.current)).toHaveLength(1);
  });

  it("does nothing for the document that is open already", async () => {
    await visitAll();
    fake.readDocument.mockClear();
    await docs.go(0);
    expect(fake.readDocument).not.toHaveBeenCalled();
    expect(docs.path).toBe("d.md");
  });

  it("does nothing for a jump past the end", async () => {
    await visitAll();
    fake.readDocument.mockClear();
    await docs.go(-9);
    await docs.go(1);
    expect(fake.readDocument).not.toHaveBeenCalled();
    expect(docs.path).toBe("d.md");
  });

  it("leaves the history where it was when the document cannot be opened", async () => {
    await visitAll();
    fake.readDocument.mockImplementation(async (path: string) => {
      if (path === "b.md") throw { kind: "notFound", subject: "b.md" };
      return view(path);
    });
    await docs.go(-2);
    expect(docs.path).toBe("d.md");
    expect(docs.trail().find((s) => s.current)?.path).toBe("d.md");
    expect(docs.canBack).toBe(true);
    expect(docs.canForward).toBe(false);
    expect(toast.current).toMatchObject({ type: "message", tone: "warn" });
    // and the trail still works for a document that is there
    await docs.go(-1);
    expect(docs.path).toBe("c.md");
  });

  it("waits for the answer when leaving the document being edited needs one, then jumps", async () => {
    await visitAll();
    let resume: (() => void) | null = null;
    docs.guard = (proceed) => {
      resume = proceed;
      return true;
    };
    await docs.go(-2);
    expect(docs.path).toBe("d.md");
    docs.guard = null;
    (resume as (() => void) | null)?.();
    await vi.waitFor(() => expect(docs.path).toBe("b.md"));
  });
});
