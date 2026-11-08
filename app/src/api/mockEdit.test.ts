import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { createMockApi } = await import("./mock");
const { MOCK_CRLF_BOM_DOC } = await import("./mockFs");
type DocumentChanged = import("./types").DocumentChanged;
type Result<T> = import("./types").Result<T>;

function value<T>(result: Result<T>): T {
  if (!result.ok) throw new Error(`refused: ${JSON.stringify(result.error)}`);
  return result.value;
}

function refusal<T>(result: Result<T>): {
  kind: string;
  subject: string | null;
  detail: string | null;
} {
  if (result.ok || result.error.kind !== "backend") throw new Error("expected a backend refusal");
  return result.error.error;
}

async function move(api: ReturnType<typeof createMockApi>, from: string, to: string, links = true) {
  const plan = value(await api.planMove([{ from, to }], links));
  return value(await api.applyMove(plan.planId, links));
}

function changes(api: ReturnType<typeof createMockApi>): DocumentChanged[] {
  const seen: DocumentChanged[] = [];
  void api.on("document-changed", (event) => seen.push(event));
  return seen;
}

// backend parity: `edit/mod.rs` save | save_as, `edit/lines.rs` apply, `document/mod.rs` (watch -> document-changed)
describe("mock editing shares the project's files", () => {
  it("a save to a renamed document's old path is a conflict with no disk version; the new path opens", async () => {
    const api = createMockApi();
    const before = value(await api.editSource("docs/guide.md"));
    await move(api, "docs/guide.md", "docs/manual.md", false);
    const saved = value(await api.editSave("docs/guide.md", before.hash, []));
    expect(saved).toEqual({ outcome: "conflict", diskHash: null });
    const moved = value(await api.editSource("docs/manual.md"));
    expect(moved.text).toBe(before.text);
    expect(moved.hash).toBe(before.hash);
    expect(refusal(await api.editSource("docs/guide.md")).kind).toBe("notFound");
  });

  it("a save to a deleted document is a conflict with no disk version", async () => {
    const api = createMockApi();
    const before = value(await api.editSource("notes/ideas.md"));
    value(await api.deleteFiles(["notes"]));
    const saved = value(await api.editSave("notes/ideas.md", before.hash, []));
    expect(saved).toEqual({ outcome: "conflict", diskHash: null });
  });

  it("a save writes the project's file: the project, the text and the next source see it", async () => {
    const api = createMockApi();
    const before = value(await api.editSource("docs/guide.md"));
    const out = value(
      await api.editSave("docs/guide.md", before.hash, [
        { at: 0, remove: 1, insert: ["# Manual"] },
      ]),
    );
    const text = api.probe.project.files.get("docs/guide.md");
    expect(text?.startsWith("# Manual\n")).toBe(true);
    expect(value(await api.documentText("docs/guide.md"))).toBe(text);
    const after = value(await api.editSource("docs/guide.md"));
    expect(out).toEqual({ outcome: "saved", hash: after.hash, mtimeMs: expect.any(Number) });
  });

  it("a link rewrite of a move changes the editor's text: a save from the old version is a conflict", async () => {
    const api = createMockApi();
    const before = value(await api.editSource("docs/guide.md"));
    expect(before.text).toContain("(../README.md)");
    const outcome = await move(api, "README.md", "START.md");
    expect(outcome.documents).toBeGreaterThan(0);
    const after = value(await api.editSource("docs/guide.md"));
    expect(after.text).toContain("(../START.md)");
    const saved = value(await api.editSave("docs/guide.md", before.hash, []));
    expect(saved).toEqual({ outcome: "conflict", diskHash: after.hash });
  });

  it("the shown document hears what file operations did to it, as from the watch", async () => {
    const api = createMockApi();
    const seen = changes(api);
    value(await api.openDocument("docs/guide.md", []));
    await move(api, "README.md", "START.md");
    const version = value(await api.editSource("docs/guide.md")).hash;
    expect(seen).toEqual([{ window: "main", path: "docs/guide.md", change: "changed", version }]);
    await move(api, "docs/guide.md", "docs/manual.md");
    expect(seen.at(-1)).toEqual({
      window: "main",
      path: "docs/guide.md",
      change: "deleted",
      movedTo: "docs/manual.md",
    });
    value(await api.openDocument("notes/ideas.md", []));
    value(await api.deleteFiles(["notes"]));
    expect(seen.at(-1)).toEqual({
      window: "main",
      path: "notes/ideas.md",
      change: "deleted",
      movedTo: null,
    });
    expect(seen).toHaveLength(3);
  });

  it("an own save is no news to the window that made it", async () => {
    const api = createMockApi();
    const seen = changes(api);
    value(await api.openDocument("docs/guide.md", []));
    const source = value(await api.editSource("docs/guide.md"));
    value(
      await api.editSave("docs/guide.md", source.hash, [{ at: 0, remove: 1, insert: ["# A"] }]),
    );
    await move(api, "notes/todo.txt", "notes/later.txt");
    expect(seen).toEqual([]);
  });

  it("the text of a document comes from its file; a missing file is not found", async () => {
    const api = createMockApi();
    expect(value(await api.documentText("README.md"))).toBe(
      api.probe.project.files.get("README.md"),
    );
    expect(refusal(await api.documentText("gone.md"))).toMatchObject({
      kind: "notFound",
      subject: "gone.md",
    });
  });
});

describe("mock save checks edits like the backend", () => {
  const cases: [string, { at: number; remove: number; insert: string[] }[], string][] = [
    ["past the end", [{ at: 999, remove: 1, insert: [] }], "outOfRange"],
    [
      "out of order",
      [
        { at: 4, remove: 1, insert: ["x"] },
        { at: 1, remove: 1, insert: ["y"] },
      ],
      "order",
    ],
    ["a line break in a line", [{ at: 0, remove: 1, insert: ["a\nb"] }], "lineBreak"],
    ["a carriage return in a line", [{ at: 0, remove: 1, insert: ["a\rb"] }], "lineBreak"],
  ];
  it.each(cases)(
    "refuses edits %s as invalid data, the file untouched",
    async (_, edits, detail) => {
      const api = createMockApi();
      const source = value(await api.editSource("docs/guide.md"));
      const text = api.probe.project.files.get("docs/guide.md");
      const result = await api.editSave("docs/guide.md", source.hash, edits);
      expect(refusal(result)).toEqual({ kind: "invalidData", subject: "docs/guide.md", detail });
      expect(api.probe.project.files.get("docs/guide.md")).toBe(text);
      expect(api.probe.edit.saves).toEqual([]);
    },
  );
});

describe("mock save as", () => {
  it("refuses an exact path that exists (any case)", async () => {
    const api = createMockApi();
    const result = await api.editSaveAs("Docs/Guide.md", "mine\n", true);
    expect(refusal(result)).toMatchObject({ kind: "exists", subject: "Docs/Guide.md" });
    expect(api.probe.project.files.get("docs/guide.md")).not.toBe("mine\n");
  });

  it("writes an exact path that is free into the project", async () => {
    const api = createMockApi();
    expect(value(await api.editSaveAs("docs/back.md", "mine\n", true))).toBe("docs/back.md");
    expect(api.probe.project.files.get("docs/back.md")).toBe("mine\n");
  });

  it("looks for a free -mine name among the project's files", async () => {
    const api = createMockApi();
    api.probe.project.add("docs/guide-mine.md", "older copy\n");
    expect(value(await api.editSaveAs("docs/guide.md", "mine\n", false))).toBe(
      "docs/guide-mine-2.md",
    );
    expect(api.probe.project.files.get("docs/guide-mine.md")).toBe("older copy\n");
  });
});

describe("mock CRLF + BOM document", () => {
  it("opens as text with LF breaks and tells its line ending and encoding", async () => {
    const api = createMockApi();
    const raw = api.probe.project.files.get(MOCK_CRLF_BOM_DOC);
    expect(raw?.startsWith("﻿")).toBe(true);
    expect(raw).toContain("\r\n");
    const source = value(await api.editSource(MOCK_CRLF_BOM_DOC));
    expect(source.text).not.toContain("\r");
    expect(source.text.startsWith("﻿")).toBe(false);
    expect(source).toMatchObject({ lineEnding: "crLf", encoding: "utf8Bom" });
    expect(value(await api.documentText(MOCK_CRLF_BOM_DOC)).startsWith("﻿")).toBe(false);
  });

  it("a save keeps the BOM and gives new lines the file's CRLF", async () => {
    const api = createMockApi();
    const raw = api.probe.project.files.get(MOCK_CRLF_BOM_DOC) ?? "";
    const source = value(await api.editSource(MOCK_CRLF_BOM_DOC));
    value(
      await api.editSave(MOCK_CRLF_BOM_DOC, source.hash, [{ at: 1, remove: 0, insert: ["added"] }]),
    );
    const lines = raw.split("\r\n");
    lines.splice(1, 0, "added");
    expect(api.probe.project.files.get(MOCK_CRLF_BOM_DOC)).toBe(lines.join("\r\n"));
  });

  it("a -mine copy takes the original's line ending and encoding", async () => {
    const api = createMockApi();
    const copy = value(await api.editSaveAs(MOCK_CRLF_BOM_DOC, "a\nb\n", false));
    expect(api.probe.project.files.get(copy)).toBe("﻿a\r\nb\r\n");
  });
});

describe("mock editor failures on request", () => {
  it.each([
    ["edit_source", "binary", null],
    ["edit_source", "encoding", null],
    ["edit_source", "tooLarge", null],
    ["edit_source", "unsupported", null],
    ["edit_save", "denied", "readOnly"],
    ["edit_save", "tooLarge", null],
  ] as const)("%s fails with %s", async (command, kind, detail) => {
    const api = createMockApi();
    const source = value(await api.editSource("docs/guide.md"));
    api.probe.edit.failures.push({ command, path: "docs/guide.md", kind, detail });
    const result =
      command === "edit_source"
        ? await api.editSource("docs/guide.md")
        : await api.editSave("docs/guide.md", source.hash, []);
    expect(result).toEqual({
      ok: false,
      error: { kind: "backend", command, error: { kind, subject: "docs/guide.md", detail } },
    });
    // another document is not touched by it
    expect(value(await api.editSource("notes/ideas.md")).path).toBe("notes/ideas.md");
  });
});
