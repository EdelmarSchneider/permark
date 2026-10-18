// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { clearReview, decorateReview, type ReviewLabels } from "./reviewLayer";
import type { ReviewBlock, ReviewChange, ReviewView } from "./types";

const labels: ReviewLabels = {
  kind: (change, index, total) => `${change.kind} ${index + 1}/${total}`,
  group: (change, index, total) => `Change ${index + 1} of ${total}, ${change.kind}`,
  removed: (count, preview) => `${count} removed: ${preview}`,
  show: "Show",
  hide: "Hide",
  keep: (index, total) => `Keep ${index + 1} of ${total}`,
  revert: (index, total) => `Revert ${index + 1} of ${total}`,
  keepHint: "Keep",
  revertHint: "Revert",
};

const block = (start: number, end: number, text: string): ReviewBlock => ({
  index: 0,
  startLine: start,
  endLine: end,
  text,
  html: `<p>${text}</p>`,
  startByte: 0,
  endByte: text.length,
});

const change = (patch: Partial<ReviewChange> & Pick<ReviewChange, "kind">): ReviewChange => ({
  old: { startByte: 0, endByte: 1 },
  new: { startByte: 0, endByte: 1 },
  oldBlocks: { start: 0, end: 1 },
  newBlocks: { start: 0, end: 1 },
  newLine: 1,
  rows: [],
  ...patch,
});

function view(changes: ReviewChange[]): ReviewView {
  return {
    path: "docs/spec.md",
    status: "changes",
    hash: "ab",
    reviewedHash: "cd",
    restorable: false,
    changes,
  };
}

function bodyOf(html: string): HTMLElement {
  const body = document.createElement("div");
  body.innerHTML = html;
  return body;
}

const paint = (body: HTMLElement, changes: ReviewChange[], current = 0) =>
  decorateReview(body, view(changes), {
    current,
    expanded: new Set(),
    labels,
    onCurrent: () => {},
    onKeep: () => {},
    onRevert: () => {},
    onToggle: () => {},
  });

describe("decorateReview", () => {
  it("wraps a modified block with the old text before the rendered one", () => {
    const body = bodyOf('<p data-sourcepos="1:1-1:5">new</p><p data-sourcepos="3:1-3:5">stay</p>');
    paint(body, [
      change({
        kind: "modified",
        newLine: 1,
        rows: [{ old: block(1, 1, "old"), new: block(1, 1, "new") }],
      }),
    ]);
    const wrapper = body.querySelector(".rv-change");
    expect(wrapper?.getAttribute("aria-label")).toBe("Change 1 of 1, modified");
    expect(wrapper?.querySelector("del")?.textContent).toBe("old");
    expect(wrapper?.querySelector("ins")?.textContent).toBe("new");
    expect(body.querySelector("p[data-sourcepos='3:1-3:5']")?.parentElement).toBe(body);
  });

  it("wraps an added block and leaves the rest alone", () => {
    const body = bodyOf('<p data-sourcepos="1:1-1:4">add</p><p data-sourcepos="3:1-3:4">old</p>');
    paint(body, [
      change({
        kind: "added",
        newLine: 1,
        rows: [{ old: null, new: block(1, 1, "add") }],
      }),
    ]);
    expect(body.querySelector(".rv-change ins")?.textContent).toBe("add");
    expect(body.querySelectorAll(".rv-change")).toHaveLength(1);
  });

  it("places a removal before the block that follows it, and one at the end last", () => {
    const body = bodyOf('<p data-sourcepos="1:1-1:4">top</p><p data-sourcepos="5:1-5:4">end</p>');
    paint(body, [
      change({
        kind: "removed",
        newLine: 3,
        old: { startByte: 10, endByte: 20 },
        rows: [{ old: block(1, 1, "gone from the middle of the text"), new: null }],
      }),
      change({
        kind: "removed",
        newLine: null,
        old: { startByte: 40, endByte: 50 },
        rows: [{ old: block(9, 9, "last"), new: null }],
      }),
    ]);
    const wrappers = [...body.querySelectorAll(".rv-change")];
    expect(wrappers).toHaveLength(2);
    expect(wrappers[0].querySelector(".rv-removed__bar")?.textContent).toContain("gone from the middle");
    expect(wrappers[0].nextElementSibling?.getAttribute("data-sourcepos")).toBe("5:1-5:4");
    expect(wrappers[1]).toBe(body.lastElementChild);
  });

  it("marks the current change and clears back to the rendered blocks", () => {
    const body = bodyOf('<p data-sourcepos="1:1-1:4">one</p><p data-sourcepos="3:1-3:4">two</p>');
    paint(
      body,
      [
        change({ kind: "added", newLine: 1, rows: [{ old: null, new: block(1, 1, "one") }] }),
        change({ kind: "added", newLine: 3, rows: [{ old: null, new: block(3, 3, "two") }] }),
      ],
      1,
    );
    expect(body.querySelector<HTMLElement>(".rv-change.is-current")?.dataset.index).toBe("1");
    clearReview(body);
    expect(body.querySelector(".rv-change")).toBeNull();
    expect(body.querySelectorAll("p")).toHaveLength(2);
  });
});
