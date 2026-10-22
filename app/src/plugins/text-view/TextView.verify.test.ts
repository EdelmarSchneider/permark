// @vitest-environment jsdom
// Verifier (integration 0.7): secrets x wrap x very long text. Directive "secrets hidden on screen by default".
// A secret on a line longer than 4000 characters stays masked, wrapping on or off (long lines scanned by windows).
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DocumentView } from "../../api";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { default: TextView } = await import("./TextView.svelte");
const { syncPlugins } = await import("../../core/plugins/instance.svelte");
const { MASK } = await import("../../core/config/secrets");
const { look } = await import("../../core/look.svelte");

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

function viewOf(text: string, path: string): DocumentView {
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

function show(view: DocumentView): void {
  target.innerHTML = "<div data-scroller></div>";
  const pane = target.firstElementChild;
  if (!(pane instanceof HTMLElement)) throw new Error("no pane");
  component = mount(TextView, { target: pane, props: { view, workspace: 1 } });
  flushSync();
}

const SECRET = "abc123secretvalue";
// a line of ~5 KB: over the 4000-character cap, drawn whole (< 200 rows), in a text long enough to be drawn by rows
// (a smaller text is drawn by the code block, which masks the whole text)
const MID = `API_TOKEN=${SECRET} ${"x".repeat(5_000)}\n${"y\n".repeat(25_000)}`;
// the same secret on a short line, in a text long enough to be drawn by rows (masked: control)
const SHORT = `API_TOKEN=${SECRET} x\n${"y\n".repeat(25_000)}`;
// one line of ~300 KB (> 200 rows at 80 columns), the secret near its start: drawn in parts when wrapping
const LONG = `API_TOKEN=${SECRET} ${"x".repeat(300_000)}\n`;

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  syncPlugins(["highlight", "text-view"]);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  look.wrap = true;
  syncPlugins([]);
});

describe("verify: a secret on a very long line", () => {
  it("control: masked on a short line", () => {
    look.wrap = true;
    show(viewOf(SHORT, "one.log"));
    const shown = [...target.querySelectorAll(".row .line")].map((l) => l.textContent).join("");
    expect(shown).not.toContain(SECRET);
  });

  it("masked on a 5 KB line when long lines do not wrap", () => {
    look.wrap = false;
    show(viewOf(MID, "one.log"));
    const shown = [...target.querySelectorAll(".row .line")].map((l) => l.textContent).join("");
    expect(shown).not.toContain(SECRET);
    expect(target.querySelector(".row .secret")?.textContent).toBe(MASK);
  });

  it("stays masked on a 300 KB line when long lines wrap (the default)", () => {
    look.wrap = true;
    show(viewOf(LONG, "one.log"));
    const shown = [...target.querySelectorAll(".row .line")].map((l) => l.textContent).join("");
    expect(shown.length).toBeGreaterThan(0);
    expect(shown).not.toContain(SECRET);
  });
});
