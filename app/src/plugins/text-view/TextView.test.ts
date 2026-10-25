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
const { look } = await import("../../core/look.svelte");
const { rowsOf } = await import("./wrapRows");

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
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  syncPlugins(["highlight", "text-view"]);
  // the cases below that do not say otherwise are about long lines that do not wrap
  look.wrap = false;
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  documents.lineGlow = null;
  documents.lineRequest = null;
  look.wrap = true;
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

describe("a line asked for (a code reference)", () => {
  afterEach(() => {
    documents.lineRequest = null;
  });

  it("is tinted a moment where it is, once per request", async () => {
    documents.lineRequest = { path: "a.txt", line: 30, seq: 901 };
    show(viewOf(lines(0, 50), null, "a.txt"));
    await until(() => target.querySelector(".located") !== null, "band");
    const band = target.querySelector<HTMLElement>(".located");
    // padding of the plain view + 29 lines above it
    expect(parseFloat(band?.style.top ?? "")).toBeCloseTo(14 + 29 * 21.6, 0);
    // gone after its moment
    await until(() => target.querySelector(".located") === null, "band gone");
  });

  it("is not asked for another file", async () => {
    documents.lineRequest = { path: "other.txt", line: 3, seq: 902 };
    show(viewOf(lines(0, 50), null, "a.txt"));
    await tick();
    expect(target.querySelector(".located")).toBeNull();
  });

  it("loads the parts of a large text until the line is in, at most ten, then says so", async () => {
    const notes: string[] = [];
    vi.spyOn(documents, "notify").mockImplementation((text) => void notes.push(text));
    // parts of 300 lines that never end: the view stops asking once the end is far (2 000 lines), the line asks ten
    let at = 100;
    const more = vi.spyOn(api, "documentMore").mockImplementation(() => {
      const chunk = lines(at, at + 300);
      at += 300;
      return ok({ text: chunk, next: 10_000 + at });
    });
    documents.lineRequest = { path: "big.log", line: 50_000, seq: 903 };
    show(viewOf(lines(0, 100), { loaded: 1000, total: 900_000_000 }));
    await until(() => notes.length > 0, "note");
    expect(more.mock.calls.length).toBeLessThanOrEqual(20);
    expect(notes[0]).toContain("50000");
    expect(target.querySelector(".located")).not.toBeNull();
  });

  it("past the end of a whole file: its last line, no note", async () => {
    const notify = vi.spyOn(documents, "notify");
    documents.lineRequest = { path: "a.txt", line: 400, seq: 904 };
    show(viewOf(lines(0, 10), null, "a.txt"));
    await until(() => target.querySelector(".located") !== null, "band");
    expect(parseFloat(target.querySelector<HTMLElement>(".located")?.style.top ?? "")).toBeCloseTo(
      14 + 9 * 21.6,
      0,
    );
    expect(notify).not.toHaveBeenCalled();
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

  it("masks no ordinary code, and every line of a private key", async () => {
    vi.spyOn(api, "documentMore").mockImplementation(() => ok({ text: "", next: null }));
    const game = [
      "var keys = { left: false, right: false, run: false, jump: false };",
      'function keyJump(code) { return code === "KeyW" || code === "KeyZ"; }',
      'document.addEventListener("keydown", function (e) {',
      "  if (keyJump(e.code)) keys.jump = true;",
      "  keys.left = false;",
      "});",
    ];
    const pem = [
      "-----BEGIN PRIVATE KEY-----",
      "MIIEvQIBADANBg",
      "  AbCdEf==",
      "-----END PRIVATE KEY-----",
    ];
    show(viewOf([...game, ...pem, ""].join("\n"), { loaded: 400, total: 4000 }));
    const rows = [...target.querySelectorAll(".row .line")].map((row) => row.textContent);
    expect(rows.slice(0, game.length)).toEqual(game);
    expect(target.querySelectorAll(".row .secret")).toHaveLength(2);
    expect(rows[game.length + 1]).toBe(MASK);
    expect(rows[game.length + 2]).toBe(`  ${MASK}`);
    expect(rows[game.length + 3]).toBe(pem[3]);
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

describe("long lines that wrap (the setting)", () => {
  beforeEach(() => {
    look.wrap = true;
  });

  it("draws a smaller file one element per line, numbered in the page, no gutter", async () => {
    show(viewOf("one\ntwo\n\nfour\n", null, "a.txt"));
    // each line keeps its own line break: a copy gives the file's lines, nothing inserted
    // (asked until it holds: the highlighted text replaces the plain one when it comes)
    const texts = (): string =>
      [...target.querySelectorAll(".code .ln")].map((l) => l.textContent).join("|");
    await until(() => texts() === "one\n|two\n|\n|four\n", "lines");
    expect(target.querySelector(".text.wrap")).not.toBeNull();
    expect(target.querySelector(".gutter")).toBeNull();
  });

  it("draws the plain text the same way with the highlight plugin off, CRLF without its carriage return", async () => {
    syncPlugins(["text-view"]);
    show(viewOf("a\r\nb\r\n", null, "crlf.txt"));
    const texts = [...target.querySelectorAll(".code .ln")].map((l) => l.textContent);
    expect(texts).toEqual(["a\n", "b\n"]);
    if (component) await unmount(component);
    component = null;
    show(viewOf("", null, "empty.txt"));
    expect(target.querySelectorAll(".code .ln")).toHaveLength(1);
  });

  it("switches back to the gutter and one block of text when the setting goes off", async () => {
    show(viewOf("one\ntwo\n", null, "a.txt"));
    await until(() => target.querySelectorAll(".code .ln").length === 2, "lines");
    look.wrap = false;
    flushSync();
    await until(() => target.querySelector(".gutter") !== null, "gutter");
    expect(target.querySelector(".gutter")?.textContent).toBe("1\n2");
    expect(target.querySelectorAll(".code .ln")).toHaveLength(0);
    expect(target.querySelector(".text.wrap")).toBeNull();
  });

  it("counts the rows of a very long text, numbers each line once", () => {
    vi.spyOn(api, "documentMore").mockImplementation(() => ok({ text: "", next: null }));
    const long = Array.from({ length: 100 }, (_, i) => `${i}`.padEnd(200, "x")).join("\n") + "\n";
    show(viewOf(long, { loaded: long.length, total: long.length * 2 }));
    // no layout here: 80 characters a row, three rows a line
    const rows = rowsOf(200, 80);
    expect(rows).toBe(3);
    const height = (target.querySelector(".virtual") as HTMLElement).style.height;
    expect(parseFloat(height)).toBeCloseTo(100 * rows * 21.6, 0);
    const numbers = [...target.querySelectorAll(".row .no")].map((n) => n.textContent);
    expect(numbers.slice(0, 3)).toEqual(["1", "2", "3"]);
    expect(target.querySelector(".row .line")?.textContent).toBe(`0${"x".repeat(199)}`);
  });

  it("draws only a few rows of a single line of 5 MB", () => {
    const line = "0123456789".repeat(500_000);
    show(viewOf(`${line}\n`, null, "one.log"));
    const total = rowsOf(5_000_000, 80);
    const height = (target.querySelector(".virtual") as HTMLElement).style.height;
    expect(parseFloat(height)).toBeCloseTo(total * 21.6, 0);
    const drawn = target.querySelectorAll(".row .line");
    expect(drawn).toHaveLength(1);
    const shown = drawn[0]?.textContent ?? "";
    expect(shown.length).toBeGreaterThan(0);
    expect(shown.length).toBeLessThan(10_000);
    expect(line.startsWith(shown)).toBe(true);
    expect(target.querySelector(".row .no")?.textContent).toBe("1");
  });

  it("places the glow and a line asked for by the rows above them", async () => {
    const long = [
      "x".repeat(200),
      "short",
      "y".repeat(200),
      ...Array.from({ length: 30_000 }, () => "z"),
    ];
    show(viewOf(`${long.join("\n")}\n`, null, "log.txt"));
    documents.lineGlow = { path: "log.txt", added: [{ first: 3, last: 3 }], removedAfter: [1] };
    flushSync();
    const glow = target.querySelector<HTMLElement>(".glow");
    // three rows, then one
    expect(parseFloat(glow?.style.top ?? "")).toBeCloseTo(4 * 21.6, 0);
    expect(parseFloat(glow?.style.height ?? "")).toBeCloseTo(3 * 21.6, 0);
    expect(parseFloat(target.querySelector<HTMLElement>(".gone")?.style.top ?? "")).toBeCloseTo(
      3 * 21.6 - 2,
      0,
    );
    documents.lineRequest = { path: "log.txt", line: 4, seq: 905 };
    flushSync();
    await until(() => target.querySelector(".located") !== null, "band");
    expect(parseFloat(target.querySelector<HTMLElement>(".located")?.style.top ?? "")).toBeCloseTo(
      7 * 21.6,
      0,
    );
  });

  it("not wrapping: the text box takes the keyboard to scroll sideways (named region)", async () => {
    look.wrap = false;
    show(viewOf("one\ntwo\n", null, "a.txt"));
    // asked until it holds: the highlighted text replaces the plain one when it comes
    await until(
      () =>
        target.querySelector(
          ".code pre[tabindex='0'][role='region'][aria-label='Text (scrolls sideways)']",
        ) !== null,
      "a focusable, named region",
    );
    look.wrap = true;
    flushSync();
    await until(
      () => target.querySelector(".code pre[tabindex]") === null,
      "no tab stop when wrapping",
    );
    if (component) await unmount(component);
    component = null;
    look.wrap = false;
    show(viewOf(lines(0, 30_000), null, "big.txt"));
    expect(target.querySelector(".virtual")?.getAttribute("tabindex")).toBe("0");
  });
});
