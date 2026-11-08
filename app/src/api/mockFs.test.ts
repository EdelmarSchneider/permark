import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { createMockApi } = await import("./mock");
const { checkName, newFileName } = await import("./mockNames");
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

// backend parity: `fileops/names.rs` check_name | new_file_name
describe("mock names", () => {
  it.each(["a.md", "My notes.md", "ação 日本語.md", ".hidden", "a b c", "x.tar.gz", "ü"])(
    "accepts %s",
    (name) => expect(checkName(name)).toBeNull(),
  );

  it.each([
    ["", "empty"],
    ["   ", "empty"],
    [".", "dot"],
    ["..", "dot"],
    ["a/b", "chars"],
    ["a\\b", "chars"],
    ["a:b", "chars"],
    ["a*b", "chars"],
    ["a?b", "chars"],
    ['a"b', "chars"],
    ["a<b", "chars"],
    ["a|b", "chars"],
    ["a\u0007b", "chars"],
    ["a\u0000b", "chars"],
    [" a", "edge"],
    ["a ", "edge"],
    ["a.", "edge"],
    ["CON", "reserved"],
    ["nul.txt", "reserved"],
    ["com1.md", "reserved"],
    ["a".repeat(256), "tooLong"],
    ["日".repeat(86), "tooLong"],
  ])("refuses %j: %s", (name, problem) => expect(checkName(name)).toBe(problem));

  it("names new files Markdown or text", () => {
    expect(newFileName("notes")).toEqual({ ok: true, name: "notes.md" });
    expect(newFileName("Notes.MD")).toEqual({ ok: true, name: "Notes.MD" });
    expect(newFileName("todo.txt")).toEqual({ ok: true, name: "todo.txt" });
    expect(newFileName("a.markdown")).toEqual({ ok: true, name: "a.markdown" });
    expect(newFileName("run.sh")).toEqual({ ok: false, problem: "notMarkdown" });
    expect(newFileName(".env")).toEqual({ ok: false, problem: "notMarkdown" });
    expect(newFileName("con")).toEqual({ ok: false, problem: "reserved" });
    expect(newFileName("a".repeat(253))).toEqual({ ok: false, problem: "tooLong" });
  });
});

// backend parity: `fileops/ops.rs` check_moves | check_deletes, `fileops/mod.rs` create | plan_links
describe("mock file operations refuse what the backend refuses", () => {
  it("creates with the backend's name rules", async () => {
    const api = createMockApi();
    expect(refusal(await api.newEntry("", "CON", "file"))).toEqual({
      kind: "invalidName",
      subject: null,
      detail: "reserved",
    });
    expect(refusal(await api.newEntry("", "a.", "folder")).detail).toBe("edge");
    expect(refusal(await api.newEntry("", "run.sh", "file")).detail).toBe("notMarkdown");
    expect(value(await api.newEntry("docs", "plan", "file"))).toBe("docs/plan.md");
  });

  it("sees a name taken in another case", async () => {
    const api = createMockApi();
    expect(api.probe.project.exists("readme.md")).toBe(true);
    expect(api.probe.project.exists("DOCS")).toBe(true);
    expect(refusal(await api.newEntry("", "readme.md", "file"))).toMatchObject({ kind: "exists" });
    expect(
      refusal(await api.planMove([{ from: "notes/ideas.md", to: "Notes/Todo.txt" }], false)),
    ).toMatchObject({
      kind: "exists",
    });
  });

  it("renames a name to another case of itself; the same name is nothing to do", async () => {
    const api = createMockApi();
    const plan = value(await api.planMove([{ from: "README.md", to: "readme.md" }], false));
    value(await api.applyMove(plan.planId, false));
    expect(api.probe.project.files.has("readme.md")).toBe(true);
    expect(api.probe.project.files.has("README.md")).toBe(false);
    expect(refusal(await api.planMove([{ from: "readme.md", to: "readme.md" }], false)).kind).toBe(
      "unavailable",
    );
  });

  it("refuses a rename to a name the rules refuse", async () => {
    const api = createMockApi();
    expect(refusal(await api.planMove([{ from: "README.md", to: "nul.md" }], false))).toEqual({
      kind: "invalidName",
      subject: null,
      detail: "reserved",
    });
  });

  it("keeps .git, .permark and the root out of moves, deletes and new entries", async () => {
    const api = createMockApi();
    api.probe.project.add(".git/HEAD", "ref");
    api.probe.project.add(".permark/settings.toml", "");
    expect(refusal(await api.deleteFiles([".git"]))).toMatchObject({
      kind: "protected",
      subject: ".git",
    });
    expect(refusal(await api.deleteFiles([".permark/settings.toml"])).kind).toBe("protected");
    expect(refusal(await api.deleteFiles([""])).kind).toBe("protected");
    expect(
      refusal(await api.planMove([{ from: "README.md", to: ".git/README.md" }], false)).kind,
    ).toBe("protected");
    expect(refusal(await api.planMove([{ from: ".git", to: "git" }], false)).kind).toBe(
      "protected",
    );
    expect(refusal(await api.newEntry(".permark", "x", "file")).kind).toBe("protected");
    expect(api.probe.project.exists(".git/HEAD")).toBe(true);
  });

  it("deletes all or nothing; an empty list is nothing to do", async () => {
    const api = createMockApi();
    expect(refusal(await api.deleteFiles(["notes/ideas.md", "gone.md"]))).toMatchObject({
      kind: "notFound",
      subject: "gone.md",
    });
    expect(api.probe.project.files.has("notes/ideas.md")).toBe(true);
    expect(refusal(await api.deleteFiles([])).kind).toBe("unavailable");
  });

  it("leaves the links of a document with unsaved edits to its editor", async () => {
    const api = createMockApi();
    const readme = api.probe.project.files.get("README.md");
    value(await api.editSetUnsaved("README.md"));
    const plan = value(await api.planMove([{ from: "docs/guide.md", to: "docs/manual.md" }], true));
    expect(plan.skipped).toEqual([{ path: "README.md", reason: "unsaved" }]);
    expect(plan.files.map((f) => f.path)).not.toContain("README.md");
    const outcome = value(await api.applyMove(plan.planId, true));
    expect(api.probe.project.files.get("README.md")).toBe(readme);
    expect(outcome.skipped).toEqual([]);
  });

  it("rewrites only what the preview showed: a document changed since is skipped", async () => {
    const api = createMockApi();
    const plan = value(await api.planMove([{ from: "docs/guide.md", to: "docs/manual.md" }], true));
    expect(plan.files.map((f) => f.path)).toContain("README.md");
    const changed = `${api.probe.project.files.get("README.md") ?? ""}more\n`;
    api.probe.project.files.set("README.md", changed);
    const outcome = value(await api.applyMove(plan.planId, true));
    expect(outcome.skipped).toEqual([{ path: "README.md", reason: "changed" }]);
    expect(api.probe.project.files.get("README.md")).toBe(changed);
  });
});
