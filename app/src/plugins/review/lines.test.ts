// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ReviewChange } from "../../api";
import {
  OLD_LINES_SHOWN,
  createLinesView,
  lineText,
  type LinesData,
  type LinesView,
} from "./lines";

function change(
  index: number,
  kind: ReviewChange["kind"],
  start: number,
  len: number,
  old: string[] = [],
  whitespace = false,
): ReviewChange {
  return { index, kind, start, len, ghosts: [], old, whitespace };
}

const TEXT = ["fn main() {", "    new();", "    kept();", "    added();", "}", ""].join("\n");
const CHANGES = [
  change(0, "edited", 1, 1, ["    old();"]),
  change(1, "added", 3, 1),
  change(2, "removed", 5, 0, ["// gone"]),
];

function data(changes: readonly ReviewChange[], current = 0, canKeep = true): LinesData {
  return { changes, total: changes.length, current, canKeep };
}

let views: LinesView[] = [];

function setup() {
  const parent = document.createElement("div");
  document.body.append(parent);
  const host = { keep: vi.fn(), revert: vi.fn() };
  const view = createLinesView({ parent, path: "src/main.rs", host });
  views.push(view);
  return { parent, view, host };
}

afterEach(() => {
  for (const view of views) view.destroy();
  views = [];
  document.body.replaceChildren();
});

describe("the text in review", () => {
  it("numbers its lines the way the backend does", () => {
    expect(lineText("a\r\nb\rc\nd")).toBe("a\nb\nc\nd");
  });

  it("tints the lines of the changes and puts a bar with the old lines before each", () => {
    const { parent, view } = setup();
    view.show(TEXT, data(CHANGES));
    // the edited and the added line are marked, the kept one is not
    const marked = [...parent.querySelectorAll(".cm-line.rv-line")].map((l) => l.textContent);
    expect(marked).toEqual(["    new();", "    added();"]);
    expect(parent.querySelectorAll(".cm-line.rv-line-edited")).toHaveLength(1);
    expect(parent.querySelectorAll(".cm-line.rv-line-added")).toHaveLength(1);
    // one bar for each change, the first is the current one
    const bars = [...parent.querySelectorAll(".rv-code-bar")];
    expect(bars).toHaveLength(3);
    expect(bars[0]?.classList.contains("is-current")).toBe(true);
    expect(bars[1]?.classList.contains("is-current")).toBe(false);
    expect(bars[0]?.querySelector(".rv-kind")?.textContent).toBe("Changed · 1 of 3");
    expect(bars[1]?.querySelector(".rv-kind")?.textContent).toBe("Added · 2 of 3");
    expect(bars[2]?.querySelector(".rv-kind")?.textContent).toBe("Removed · 3 of 3");
    // the old lines of the first and of the removal
    const old = [...parent.querySelectorAll(".rv-old-text")].map((l) => l.textContent);
    expect(old).toEqual(["    old();", "// gone"]);
    // the removal sits after the last line
    const hunks = [...parent.querySelectorAll(".rv-hunk")];
    expect(hunks.at(-1)?.getAttribute("data-rv-change")).toBe("2");
  });

  it("calls keep and revert for the change whose button was clicked", () => {
    const { parent, view, host } = setup();
    view.show(TEXT, data(CHANGES));
    const buttons = (index: number): HTMLButtonElement[] => [
      ...parent.querySelectorAll<HTMLButtonElement>(`[data-rv-change="${index}"] button`),
    ];
    expect(buttons(1).map((b) => b.textContent)).toEqual(["Keep", "Revert"]);
    buttons(1)[0]?.click();
    buttons(2)[1]?.click();
    expect(host.keep).toHaveBeenCalledWith(1);
    expect(host.revert).toHaveBeenCalledWith(2);
  });

  it("offers no Keep when the base is the last commit", () => {
    const { parent, view } = setup();
    view.show(TEXT, data(CHANGES, 0, false));
    const labels = [...parent.querySelectorAll(".rv-code-bar button")].map((b) => b.textContent);
    expect(labels).toEqual(["Revert", "Revert", "Revert"]);
  });

  it("moves the current mark when the keys move on, and keeps the other bars as they were", () => {
    const { parent, view } = setup();
    view.show(TEXT, data(CHANGES, 0));
    const second = parent.querySelectorAll(".rv-code-bar")[1];
    view.show(TEXT, data(CHANGES, 1));
    const bars = parent.querySelectorAll(".rv-code-bar");
    expect(bars[0]?.classList.contains("is-current")).toBe(false);
    expect(bars[1]?.classList.contains("is-current")).toBe(true);
    expect(parent.querySelectorAll(".cm-line.is-current")).toHaveLength(1);
    expect(second).toBeDefined();
  });

  it("says when the old and the new differ in whitespace only", () => {
    const { parent, view } = setup();
    view.show(TEXT, data([change(0, "edited", 1, 1, ["\tnew();"], true)]));
    expect(parent.querySelector(".rv-ws")?.textContent).toBe("whitespace only");
  });

  it("shows a text that was replaced, and the changes of the new one", () => {
    const { parent, view } = setup();
    view.show(TEXT, data(CHANGES));
    view.show("one\ntwo\n", data([change(0, "added", 1, 1)]));
    expect(parent.querySelectorAll(".cm-line.rv-line")).toHaveLength(1);
    expect(parent.querySelectorAll(".rv-code-bar")).toHaveLength(1);
    expect(parent.querySelector(".cm-content")?.textContent).not.toContain("kept");
  });

  it("draws only the first old lines of a rewrite, and counts the rest", () => {
    const { parent, view } = setup();
    const old = Array.from({ length: OLD_LINES_SHOWN + 25 }, (_, i) => `old ${i}`);
    view.show("new\n", data([change(0, "edited", 0, 1, old)]));
    expect(parent.querySelectorAll(".rv-old-line")).toHaveLength(OLD_LINES_SHOWN);
    expect(parent.querySelector(".rv-old-more")?.textContent).toBe("… and 25 more old lines");
  });

  it("is read-only: nobody types into it", () => {
    const { parent, view } = setup();
    view.show(TEXT, data(CHANGES));
    expect(parent.querySelector(".cm-content")?.getAttribute("contenteditable")).toBe("false");
  });
});

describe("values in review", () => {
  it("show as written in the text and in the old lines", () => {
    const { parent, view } = setup();
    const text = 'name = "x"\nAPI_TOKEN=new-value\n';
    view.show(text, data([change(0, "edited", 1, 1, ["API_TOKEN=old-value"])]));
    expect(parent.querySelector(".rv-old-text")?.textContent).toBe("API_TOKEN=old-value");
    expect(parent.querySelector(".cm-content")?.textContent).toContain("API_TOKEN=new-value");
    expect(parent.querySelector(".cm-line button")).toBeNull();
  });
});
