// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DocumentView, Result, TextChunk } from "../../api";

// the viewer talks to the mock backend (one per test file)
vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { default: TextView } = await import("./TextView.svelte");
const { documents } = await import("../../core/doc/document.svelte");
const { syncPlugins } = await import("../../core/plugins/instance.svelte");
const { MASK } = await import("../../core/config/secrets");

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

function viewOf(
  text: string,
  partial: { loaded: number; total: number } | null = null,
  path = "big.log",
): DocumentView {
  return {
    path,
    name: path,
    format: "text",
    role: null,
    body: { kind: "text", text, language: "log", ...(partial ? { partial } : {}) },
    hash: partial ? null : "0123456789abcdef",
    size: partial?.total ?? text.length,
    mtimeMs: 1234,
    editable: { editable: "readOnly", reason: "tooLarge" },
    notes: [],
  };
}

const lines = (from: number, to: number): string =>
  Array.from({ length: to - from }, (_, i) => `line ${from + i}`).join("\n") + "\n";

const ok = (value: TextChunk): Promise<Result<TextChunk>> => Promise.resolve({ ok: true, value });

function show(view: DocumentView): void {
  // the pane that scrolls: the viewer finds it by its attribute
  target.innerHTML = "<div data-scroller></div>";
  const pane = target.firstElementChild;
  if (!(pane instanceof HTMLElement)) throw new Error("no pane");
  component = mount(TextView, { target: pane, props: { view, workspace: 1 } });
  flushSync();
}

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 3000, interval: 10 });
}

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  syncPlugins(["highlight", "text-view"]);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  documents.lineGlow = null;
  syncPlugins([]);
  vi.restoreAllMocks();
});

describe("a text file", () => {
  it("shows line numbers beside the text", async () => {
    show(viewOf("one\ntwo\nthree\n", null, "a.txt"));
    await until(() => target.querySelector(".gutter") !== null, "gutter");
    expect(target.querySelector(".gutter")?.textContent).toBe("1\n2\n3");
    await until(() => (target.querySelector(".code")?.textContent ?? "").includes("two"), "text");
  });

  it("numbers an empty file's one line", async () => {
    show(viewOf("", null, "empty.txt"));
    await until(() => target.querySelector(".gutter") !== null, "gutter");
    expect(target.querySelector(".gutter")?.textContent).toBe("1");
  });
});

describe("a very large file", () => {
  it("shows its first part at once as plain rows, and asks for the rest", async () => {
    const more = vi
      .spyOn(api, "documentMore")
      .mockImplementation(() => ok({ text: lines(100, 250), next: null }));
    show(viewOf(lines(0, 100), { loaded: 1000, total: 4000 }));
    // the rows are there before any answer
    expect(target.querySelectorAll(".row").length).toBeGreaterThan(50);
    expect(target.querySelector(".row .line")?.textContent).toBe("line 0");
    expect(target.querySelector(".row .no")?.textContent).toBe("1");
    // the lines in view are close to the end of what is held: the next chunk is asked for, with what the view knows
    await until(() => more.mock.calls.length === 1, "the request");
    expect(more).toHaveBeenCalledWith("big.log", 1000, 4000, 1234);
    await until(
      () => (target.querySelector(".note")?.textContent ?? "").includes("whole file"),
      "done",
    );
    // the height of the file now counts all the lines
    const height = (target.querySelector(".virtual") as HTMLElement).style.height;
    expect(parseFloat(height)).toBeCloseTo(250 * 21.6, 0);
  });

  it("joins a line that was cut between two chunks", async () => {
    vi.spyOn(api, "documentMore").mockImplementation(() =>
      ok({ text: "tail\nnext\n", next: null }),
    );
    show(viewOf("first\nsplit-hea", { loaded: 15, total: 40 }));
    await until(() => (target.querySelector(".note")?.textContent ?? "").includes("whole"), "done");
    const texts = [...target.querySelectorAll(".row .line")].map((l) => l.textContent);
    expect(texts).toEqual(["first", "split-heatail", "next"]);
  });

  it("says when the next part could not come, and tries again when asked", async () => {
    const more = vi
      .spyOn(api, "documentMore")
      .mockImplementationOnce(() =>
        Promise.resolve({
          ok: false,
          error: {
            kind: "backend",
            command: "document_more",
            error: { kind: "io", subject: "x", detail: null },
          },
        }),
      )
      .mockImplementation(() => ok({ text: lines(100, 120), next: null }));
    show(viewOf(lines(0, 100), { loaded: 500, total: 900 }));
    await until(() => target.querySelector(".note button") !== null, "the retry button");
    expect(target.querySelector(".note")?.textContent).toContain("Couldn't load");
    (target.querySelector(".note button") as HTMLElement).click();
    await until(() => (target.querySelector(".note")?.textContent ?? "").includes("whole"), "done");
    expect(more).toHaveBeenCalledTimes(2);
  });

  it("opens the file again when it is not the one that was opened", async () => {
    const reload = vi.spyOn(documents, "reload").mockImplementation(() => {});
    vi.spyOn(api, "documentMore").mockImplementation(() =>
      Promise.resolve({
        ok: false,
        error: {
          kind: "backend",
          command: "document_more",
          error: { kind: "conflict", subject: "big.log", detail: "changed" },
        },
      }),
    );
    show(viewOf(lines(0, 100), { loaded: 500, total: 900 }));
    await until(() => reload.mock.calls.length > 0, "reload");
    expect(target.querySelector(".note button")).toBeNull();
  });

  it("says how much of the file it shows, and asks for nothing while the end is far", async () => {
    const more = vi.spyOn(api, "documentMore");
    // the lines in view are far from the end of what is held: nothing is asked for
    show(viewOf(lines(0, 5000), { loaded: 500, total: 3 * 1024 * 1024 * 1024 }));
    await tick();
    expect(more).not.toHaveBeenCalled();
    expect(target.querySelector(".note")?.textContent).toMatch(/first 1 MB of 3072 MB/);
  });

  it("masks the values of secret keys until one is clicked", async () => {
    vi.spyOn(api, "documentMore").mockImplementation(() => ok({ text: "", next: null }));
    show(viewOf("name = x\nAPI_TOKEN=abc123secret\nplain\n", { loaded: 40, total: 4000 }));
    const rows = target.querySelectorAll(".row .line");
    expect(rows[0]?.textContent).toBe("name = x");
    expect(rows[1]?.textContent).toContain("API_TOKEN=");
    expect(rows[1]?.textContent).not.toContain("abc123secret");
    const mask = target.querySelector<HTMLButtonElement>(".row .secret");
    expect(mask?.textContent).toBe(MASK);
    mask?.click();
    flushSync();
    expect(target.querySelectorAll(".row .line")[1]?.textContent).toBe("API_TOKEN=abc123secret");
    expect(target.querySelector(".row .secret")).toBeNull();
  });
});

describe("lines that changed on disk", () => {
  it("glow where they are, a thin mark shows where lines went", async () => {
    show(viewOf(lines(0, 30000), null, "log.txt"));
    documents.lineGlow = {
      path: "log.txt",
      added: [{ first: 3, last: 4 }],
      removedAfter: [10],
    };
    flushSync();
    const glow = target.querySelector<HTMLElement>(".glow");
    expect(glow).not.toBeNull();
    expect(parseFloat(glow?.style.top ?? "")).toBeCloseTo(2 * 21.6, 0);
    expect(parseFloat(glow?.style.height ?? "")).toBeCloseTo(2 * 21.6, 0);
    expect(target.querySelectorAll(".gone")).toHaveLength(1);
    // another document's glow is not drawn here
    documents.lineGlow = { path: "other.txt", added: [{ first: 1, last: 1 }], removedAfter: [] };
    flushSync();
    expect(target.querySelector(".glow")).toBeNull();
  });

  it("are drawn over the highlighted text of a smaller file too", async () => {
    show(viewOf("a\nb\nc\n", null, "small.txt"));
    documents.lineGlow = { path: "small.txt", added: [{ first: 2, last: 2 }], removedAfter: [] };
    flushSync();
    const glow = target.querySelector<HTMLElement>(".code .glow");
    expect(glow).not.toBeNull();
    // 14 px of padding above the first line
    expect(parseFloat(glow?.style.top ?? "")).toBeCloseTo(14 + 21.6, 0);
  });
});
