// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DocumentView, Result, TextChunk } from "../../api";
import { linesOf } from "../../core/doc/largeText";
import { Box } from "../../core/testing/box.svelte";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

vi.mock("./json", async () => {
  const actual = await vi.importActual<typeof import("./json")>("./json");
  return { ...actual, parseJson: vi.fn(actual.parseJson) };
});

const { api } = await import("../../api");
const { parseJson } = await import("./json");
const { default: JsonlView } = await import("./JsonlView.svelte");
const { ContributionRegistry, PluginHost, pluginLoaders } =
  await import("../../core/plugins/registry.svelte");
const { heldText, recordsOf } = await import("./jsonl");

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

function viewOf(
  text: string,
  partial: { loaded: number; total: number } | null = null,
  path = "session.jsonl",
): DocumentView {
  return {
    path,
    name: path,
    format: "jsonl",
    role: null,
    body: { kind: "text", text, language: "jsonl", ...(partial ? { partial } : {}) },
    hash: partial ? null : "0123456789abcdef",
    size: partial?.total ?? text.length,
    mtimeMs: 1234,
    editable: { editable: "readOnly", reason: "tooLarge" },
    notes: [],
  };
}

const ok = (value: TextChunk): Result<TextChunk> => ({ ok: true, value });

function show(view: DocumentView): HTMLElement {
  target.innerHTML = "<div data-scroller></div>";
  const pane = target.firstElementChild;
  if (!(pane instanceof HTMLElement)) throw new Error("no pane");
  component = mount(JsonlView, { target: pane, props: { view, workspace: 1 } });
  flushSync();
  return pane;
}

/** The pane keeps whatever `scrollTop` is set, and reports it to the viewer. */
function heldPane(): HTMLElement {
  target.innerHTML = "<div data-scroller></div>";
  const pane = target.firstElementChild;
  if (!(pane instanceof HTMLElement)) throw new Error("no pane");
  let y = 0;
  Object.defineProperty(pane, "scrollTop", {
    configurable: true,
    get: () => y,
    set: (value: number) => {
      y = value;
    },
  });
  return pane;
}

function mountView(pane: HTMLElement, view: DocumentView | Box<DocumentView>): void {
  component = mount(JsonlView, {
    target: pane,
    props:
      view instanceof Box
        ? {
            get view() {
              return view.value;
            },
            workspace: 1,
          }
        : { view, workspace: 1 },
  });
  flushSync();
}

function modeButton(name: string): HTMLButtonElement {
  const button = [...target.querySelectorAll<HTMLButtonElement>("button")].find(
    (item) => item.textContent === name,
  );
  if (!button) throw new Error(name);
  return button;
}

function choose(name: string): void {
  modeButton(name).click();
  flushSync();
}

/** Far enough that the record window would leave the first records if Source scroll counted. */
function scrollPane(pane: HTMLElement): void {
  pane.scrollTop = 50_000;
  pane.dispatchEvent(new Event("scroll"));
  flushSync();
}

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

function recordsText(n: number): string {
  return Array.from({ length: n }, (_, i) => `{"n":${i + 1}}`).join("\n") + "\n";
}

/** The same cases as `format::json_lines::records` on a finished file (`lastFinal`). */
function finished(text: string): ReturnType<typeof recordsOf> {
  return recordsOf(linesOf(text).lines, true);
}

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  vi.mocked(parseJson).mockClear();
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  vi.restoreAllMocks();
});

describe("the same record boundaries as format::records", () => {
  it("skips blanks, treats CR LF as one break, and indexes the first value as 1", () => {
    const text = '\n\t \r\n{"a":1}\r\n\n  {"b": 2}';
    expect(finished(text)).toEqual([
      { index: 1, line: 3, text: '{"a":1}' },
      { index: 2, line: 5, text: '  {"b": 2}' },
    ]);
    expect(finished('{"a":1}')).toEqual([{ index: 1, line: 1, text: '{"a":1}' }]);
    expect(heldText(linesOf(text).lines, linesOf(text).open)).toBe(text);
  });

  it("has no records for an empty or blank file", () => {
    for (const text of ["", "\n", " \t\r\n\r\n", "\r"]) expect(finished(text)).toEqual([]);
    expect(finished("\u{00a0}")).toEqual([{ index: 1, line: 1, text: "\u{00a0}" }]);
  });

  it("does not join lines and does not parse JSON", () => {
    const text = '{"a":\n1}{"b":2}\n123456789012345678901234567890\n';
    expect(finished(text)).toEqual([
      { index: 1, line: 1, text: '{"a":' },
      { index: 2, line: 2, text: '1}{"b":2}' },
      { index: 3, line: 3, text: "123456789012345678901234567890" },
    ]);
  });

  it("keeps a leading BOM and only one trailing CR", () => {
    expect(finished('\u{feff}{"a":1}\n')[0]?.text).toBe('\u{feff}{"a":1}');
    expect(finished('{"a":1}\r\r\n')[0]?.text).toBe('{"a":1}\r');
  });

  it("leaves an unfinished last line out until the load is final", () => {
    const { lines, open } = linesOf('{"a":1}\n{"b":');
    expect(open).toBe(true);
    expect(recordsOf(lines, false)).toEqual([{ index: 1, line: 1, text: '{"a":1}' }]);
    expect(recordsOf(lines, true)).toEqual([
      { index: 1, line: 1, text: '{"a":1}' },
      { index: 2, line: 2, text: '{"b":' },
    ]);
  });
});

describe("the plugin", () => {
  it("claims JSON Lines beside JSON and leaves both when off", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    await host.enable("json-view");
    const viewers = registry.of("viewer").filter((v) => v.owner === "json-view");
    expect(viewers.map((v) => v.id).sort()).toEqual(["json", "jsonl"]);
    const jsonl = viewers.find((v) => v.id === "jsonl");
    const json = viewers.find((v) => v.id === "json");
    for (const name of ["session.JSONL", "data/a.ndjson", "set.jsonlines"]) {
      expect(jsonl?.claims(name), name).toBe(true);
      expect(json?.claims(name), name).toBe(false);
    }
    expect(jsonl?.claims("a.json")).toBe(false);
    expect(jsonl?.claims("a.jsonc")).toBe(false);
    expect(json?.claims("a.json")).toBe(true);
    expect(host.disable("json-view")).toBe(0);
    expect(registry.items).toHaveLength(0);
  });
});

describe("a JSON Lines file", () => {
  it("keeps a big integer's digits on that one line", () => {
    const digits = "123456789012345678901234567890";
    show(viewOf(`{"n":${digits}}\n`));
    expect(target.textContent).toContain(digits);
    expect(vi.mocked(parseJson).mock.calls.map((call) => call[0])).toEqual([`{"n":${digits}}`]);
  });

  it("marks a line with a second value, and still shows its neighbors", () => {
    show(viewOf('{"a":1}\n\n{"b":1}{"c":2}\n{"d":3}\n'));
    const bad = target.querySelector<HTMLElement>("[data-record='2']");
    expect(bad?.dataset.line).toBe("3");
    expect(bad?.querySelector(".label")).toBeNull();
    expect(bad?.textContent).toContain("Line 3");
    expect(target.querySelector("[data-record='1']")?.textContent).toContain("a");
    expect(target.querySelector("[data-record='3']")?.textContent).toContain("d");
    expect(target.querySelector(".fail")).not.toBeNull();
  });

  it("does not join a value that spans lines", () => {
    show(viewOf('{"a":\n1}\n'));
    expect(target.querySelectorAll(".fail")).toHaveLength(2);
    expect(target.querySelector("[data-line='1'] .label")).toBeNull();
    expect(target.querySelector("[data-record='1']")?.textContent).toContain("Line 1");
    expect(target.querySelector("[data-record='2']")?.textContent).toContain("Line 2");
  });

  it("does not show an unfinished last line as a failure, then reads it once the chunk ends", async () => {
    let release: (value: Result<TextChunk>) => void = () => {};
    vi.spyOn(api, "documentMore").mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );
    show(viewOf('{"a":1}\n{"b":', { loaded: 12, total: 40 }));
    await until(() => vi.mocked(api.documentMore).mock.calls.length === 1, "the next chunk");
    expect(target.querySelector(".fail")).toBeNull();
    expect(target.querySelectorAll("[data-record]")).toHaveLength(1);
    expect(target.querySelector("[data-record='1']")?.textContent).toContain("a");
    release(ok({ text: "2}\n", next: null }));
    await until(() => target.querySelector("[data-record='2']") !== null, "the finished line");
    expect(target.querySelector(".fail")).toBeNull();
    expect(target.querySelector("[data-record='2']")?.textContent).toContain("2");
  });

  it("shows an empty state for an empty file or only blank lines, not an error", () => {
    show(viewOf("\n \t\r\n"));
    expect(target.querySelector(".empty")?.textContent).toContain("No records");
    expect(target.querySelector(".fail")).toBeNull();
    expect(target.querySelector("[data-record]")).toBeNull();
  });

  it("shows the held text one click away", async () => {
    const text = '{"a":1}\n{"b":1}{"c":2}\n';
    show(viewOf(text));
    const source = [...target.querySelectorAll<HTMLButtonElement>("button")].find(
      (button) => button.textContent === "Source",
    );
    source?.click();
    await tick();
    expect(target.querySelector(".source")?.textContent).toBe(text);
    expect(target.querySelector("[data-record]")).toBeNull();
  });

  it("keeps Source when the same path arrives as a new view", () => {
    const text = recordsText(80).replace('{"n":80}', '{"mark":"LAST-RECORD-MARKER"}');
    const box = new Box(viewOf(text));
    const pane = heldPane();
    mountView(pane, box);
    expect(target.querySelector("[data-record='1']")).not.toBeNull();
    choose("Source");
    expect(modeButton("Source").getAttribute("aria-checked")).toBe("true");
    scrollPane(pane);
    box.value = { ...viewOf(text), mtimeMs: 5678 };
    flushSync();
    expect(modeButton("Source").getAttribute("aria-checked")).toBe("true");
    expect(target.querySelector(".source")?.textContent).toBe(text);
    expect(target.querySelector("[data-record]")).toBeNull();
    choose("Tree");
    expect(target.querySelector("[data-record='1']")).not.toBeNull();
    expect(target.querySelector("[data-record='80']")).toBeNull();
    expect(target.textContent).not.toContain("LAST-RECORD-MARKER");
  });

  it("a scroll while Source is showing keeps the records that were in view", () => {
    const text = recordsText(80).replace('{"n":80}', '{"mark":"LAST-RECORD-MARKER"}');
    const pane = heldPane();
    mountView(pane, viewOf(text));
    const built = (): string[] => vi.mocked(parseJson).mock.calls.map((call) => call[0] ?? "");
    const first = built();
    expect(first.length).toBeGreaterThan(0);
    expect(first.some((line) => line.includes("LAST-RECORD-MARKER"))).toBe(false);
    expect(target.querySelector("[data-record='1']")).not.toBeNull();
    choose("Source");
    scrollPane(pane);
    expect(built()).toEqual(first);
    expect(target.querySelector(".source")).not.toBeNull();
    choose("Tree");
    expect(built().slice(first.length)).toEqual(first);
    expect(target.querySelector("[data-record='1']")).not.toBeNull();
    expect(target.querySelector("[data-record='80']")).toBeNull();
    expect(target.textContent).not.toContain("LAST-RECORD-MARKER");
  });

  it("builds trees only for the records in view", () => {
    show(viewOf(recordsText(80).replace('{"n":80}', '{"mark":"LAST-RECORD-MARKER"}')));
    expect(target.querySelectorAll("[data-record]").length).toBeLessThan(40);
    expect(target.querySelector("[data-record='1']")).not.toBeNull();
    expect(target.querySelector("[data-record='80']")).toBeNull();
    expect(target.textContent).not.toContain("LAST-RECORD-MARKER");
    const parsed = vi.mocked(parseJson).mock.calls.map((call) => call[0]);
    expect(parsed.length).toBeGreaterThan(0);
    expect(parsed.length).toBeLessThan(40);
    expect(parsed.some((line) => line.includes("LAST-RECORD-MARKER"))).toBe(false);
  });

  it("does not move for a record number below 1 or past the loaded records", async () => {
    const more = vi.spyOn(api, "documentMore");
    const pane = show(viewOf('{"a":1}\n{"b":2}\n'));
    const input = target.querySelector("input");
    if (!(input instanceof HTMLInputElement)) throw new Error("no field");
    const form = target.querySelector("form");
    if (!(form instanceof HTMLFormElement)) throw new Error("no form");
    for (const value of ["0", "9", "1.5", ""]) {
      input.value = value;
      input.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      form.requestSubmit();
      await tick();
      expect(pane.scrollTop, value).toBe(0);
    }
    expect(more).not.toHaveBeenCalled();
    expect(target.querySelector("[data-record='9']")).toBeNull();
  });

  it("loads further chunks until the record, and does not move when the file ends first", async () => {
    const more = vi
      .spyOn(api, "documentMore")
      .mockImplementation(() => Promise.resolve(ok({ text: '{"n":3001}\n', next: null })));
    const pane = show(viewOf(recordsText(3000), { loaded: 3000, total: 9000 }));
    await tick();
    expect(more).not.toHaveBeenCalled();
    const input = target.querySelector("input");
    if (!(input instanceof HTMLInputElement)) throw new Error("no field");
    const form = target.querySelector("form");
    if (!(form instanceof HTMLFormElement)) throw new Error("no form");
    input.value = "3002";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    form.requestSubmit();
    await until(() => more.mock.calls.length === 1, "one chunk");
    await tick();
    expect(pane.scrollTop).toBe(0);
    expect(target.querySelector("[data-record='3001']")).toBeNull();
    expect(target.querySelector("[data-record='1']")).not.toBeNull();

    input.value = "3001";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    form.requestSubmit();
    await until(() => target.querySelector("[data-record='3001']") !== null, "record 3001");
    expect(target.querySelector("[data-record='1']")).toBeNull();
  });
});
