// @vitest-environment jsdom
// A line asked for (a code reference) in a smaller text file whose long lines wrap (real-input wrap/W6, 0.7.0): the
// pane must land on the line, wrapped rows above counted, also while the highlighter is still on its way. jsdom has
// no layout: rows are laid out here by their length (80 characters a row) and the pane cannot scroll past what is
// drawn, as in a browser.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { DocumentView } from "../../api";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
const { default: TextView } = await import("./TextView.svelte");
const { documents } = await import("../../core/doc/document.svelte");
const { contributions } = await import("../../core/plugins/instance.svelte");
const { NO_REQUIREMENT } = await import("../../core/conditions/available");
const { look } = await import("../../core/look.svelte");

const ROW = 21.6;
const PAD = 14;
const COLS = 80;
const PANE = 600;

/** 177 lines like an agent's log: every third one long, line 50 very long (63 rows). */
const LOG = Array.from({ length: 177 }, (_, i) =>
  i === 49 ? "w".repeat(5000) : i % 3 === 0 ? `${i + 1} ${"x".repeat(300)}` : `line ${i + 1}`,
);
const rowsOfLine = (text: string): number => Math.max(1, Math.ceil(text.length / COLS));
/** Pixels from the top of the text to line `n` (1-based). */
const topOf = (n: number): number =>
  PAD + LOG.slice(0, n - 1).reduce((sum, line) => sum + rowsOfLine(line) * ROW, 0);

let target: HTMLElement;
let pane: HTMLElement;
let component: Record<string, unknown> | null = null;
let scrolled = 0;
let removeCode: (() => void) | null = null;

/** The highlighter's view, coming late (as a chunk loaded on first use does in the app). */
function slowHighlighter(): void {
  removeCode = contributions.register({
    kind: "embed",
    id: "test.code",
    language: "@code",
    requires: NO_REQUIREMENT,
    owner: "highlight",
    view: async () => {
      await new Promise((resolve) => setTimeout(resolve, 300));
      return import("../highlight/CodeBlock.svelte");
    },
  });
}

function viewOf(path = "run2-G4.log"): DocumentView {
  const text = `${LOG.join("\n")}\n`;
  return {
    path,
    name: path,
    format: "text",
    role: null,
    body: { kind: "text", text, language: "log" },
    hash: "0123456789abcdef",
    size: text.length,
    mtimeMs: 1234,
    editable: { editable: "readOnly", reason: "tooLarge" },
    notes: [],
  };
}

/** The drawn lines and their heights: what the pane can scroll over. */
function drawnRows(): HTMLElement[] {
  return [...pane.querySelectorAll<HTMLElement>(".code .ln")];
}
const heightOf = (ln: Element): number =>
  rowsOfLine((ln.textContent ?? "").replace(/\n$/, "")) * ROW;
const maxScroll = (): number => {
  const rows = drawnRows();
  const content = rows.length === 0 ? 0 : 2 * PAD + rows.reduce((s, r) => s + heightOf(r), 0);
  return Math.max(0, content - PANE);
};

function rect(top: number, height: number): DOMRect {
  return {
    top,
    height,
    bottom: top + height,
    left: 0,
    right: 800,
    width: 800,
    x: 0,
    y: top,
  } as DOMRect;
}

function show(view: DocumentView): void {
  target.innerHTML = "<div data-scroller></div>";
  const first = target.firstElementChild;
  if (!(first instanceof HTMLElement)) throw new Error("no pane");
  pane = first;
  scrolled = 0;
  Object.defineProperty(pane, "clientHeight", { configurable: true, get: () => PANE });
  Object.defineProperty(pane, "scrollTop", {
    configurable: true,
    get: () => scrolled,
    set: (value: number) => (scrolled = Math.min(Math.max(0, value), maxScroll())),
  });
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
  slowHighlighter();
  look.wrap = true;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
    this: HTMLElement,
  ) {
    if (this === pane) return rect(0, PANE);
    if (this.classList.contains("code")) return rect(-scrolled, 2 * PAD + 10_000);
    if (this.classList.contains("ln")) {
      const rows = drawnRows();
      const i = rows.indexOf(this);
      const above = rows.slice(0, i).reduce((s, r) => s + heightOf(r), 0);
      return rect(-scrolled + PAD + above, heightOf(this));
    }
    return rect(0, 0);
  });
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  documents.lineRequest = null;
  removeCode?.();
  removeCode = null;
  vi.restoreAllMocks();
});

const band = (): HTMLElement | null => target.querySelector<HTMLElement>(".located");

it("draws the wrapped lines at once, before the highlighter comes (the page has its height)", () => {
  show(viewOf());
  expect(drawnRows()).toHaveLength(177);
  expect(maxScroll()).toBeGreaterThan(topOf(120));
  // a place set now (a line asked for, the place left) holds
  pane.scrollTop = topOf(87);
  expect(pane.scrollTop).toBeCloseTo(topOf(87), 0);
});

it("a file opened fresh at a line: the line in the top third, wrapped rows above counted, tinted", async () => {
  documents.lineRequest = { path: "run2-G4.log", line: 120, seq: 1001 };
  show(viewOf());
  await until(() => band() !== null, "band");
  expect(pane.scrollTop).toBeCloseTo(topOf(120) - PANE / 3, 0);
  expect(parseFloat(band()?.style.top ?? "")).toBeCloseTo(topOf(120), 0);
  // far below where unwrapped rows would put it
  expect(topOf(120)).toBeGreaterThan(PAD + 119 * ROW + 1000);
});

it("a file visited before (the pane left further down): the line asked for wins", async () => {
  show(viewOf());
  pane.scrollTop = topOf(87);
  documents.lineRequest = { path: "run2-G4.log", line: 120, seq: 1002 };
  flushSync();
  await until(() => band() !== null, "band");
  expect(pane.scrollTop).toBeCloseTo(topOf(120) - PANE / 3, 0);
});

it("a line right after a very long one, the last line, a line past the end", async () => {
  show(viewOf());
  documents.lineRequest = { path: "run2-G4.log", line: 51, seq: 1003 };
  flushSync();
  await until(() => band() !== null, "band 51");
  expect(pane.scrollTop).toBeCloseTo(topOf(51) - PANE / 3, 0);
  // the band covers at most four rows
  expect(parseFloat(band()?.style.height ?? "")).toBeLessThanOrEqual(4 * ROW + 0.01);

  documents.lineRequest = { path: "run2-G4.log", line: 177, seq: 1004 };
  flushSync();
  await until(() => parseFloat(band()?.style.top ?? "") > topOf(170), "band 177");
  expect(parseFloat(band()?.style.top ?? "")).toBeCloseTo(topOf(177), 0);
  expect(pane.scrollTop).toBeCloseTo(Math.min(topOf(177) - PANE / 3, maxScroll()), 0);

  const notify = vi.spyOn(documents, "notify");
  documents.lineRequest = { path: "run2-G4.log", line: 400, seq: 1005 };
  flushSync();
  await new Promise((resolve) => setTimeout(resolve, 50));
  expect(parseFloat(band()?.style.top ?? "")).toBeCloseTo(topOf(177), 0);
  expect(notify).not.toHaveBeenCalled();
});

it("shows the plain lines as written: while the highlighter comes, and with it off", async () => {
  const text = ["API_TOKEN=abcdef0123456789abcdef", "plain", ""].join("\n");
  const plainView = {
    ...viewOf("a.env.log"),
    body: { kind: "text" as const, text, language: "log" },
  };
  show(plainView);
  expect(pane.textContent).toContain("plain");
  expect(pane.textContent).toContain("API_TOKEN=abcdef0123456789abcdef");
  if (component) await unmount(component);
  component = null;
  // the highlighter off: the plain lines are all there is
  removeCode?.();
  removeCode = null;
  show(plainView);
  expect(pane.textContent).toContain("API_TOKEN=abcdef0123456789abcdef");
  look.wrap = false;
  flushSync();
  expect(pane.textContent).toContain("API_TOKEN=abcdef0123456789abcdef");
  expect(pane.textContent).toContain("plain");
});
