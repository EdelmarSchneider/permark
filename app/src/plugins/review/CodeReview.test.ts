// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DocumentView, ReviewChange, ReviewView } from "../../api";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { default: CodeReview } = await import("./CodeReview.svelte");
const { review } = await import("./store.svelte");

const TEXT = ["fn main() {", "    new();", "    kept();", "}", ""].join("\n");

function textView(hash: string): DocumentView {
  return {
    path: "src/main.rs",
    name: "main.rs",
    format: "text",
    role: null,
    body: { kind: "text", text: TEXT, language: "rs" },
    hash,
    size: TEXT.length,
    mtimeMs: 1,
    editable: { editable: "surgical" },
    notes: [],
  };
}

const CHANGES: ReviewChange[] = [
  {
    index: 0,
    kind: "edited",
    start: 1,
    len: 1,
    ghosts: [],
    old: ["    old();"],
    whitespace: false,
  },
  {
    index: 1,
    kind: "removed",
    start: 3,
    len: 0,
    ghosts: [],
    old: ["    // gone"],
    whitespace: false,
  },
];

function reviewView(hash: string): ReviewView {
  return {
    path: "src/main.rs",
    hash,
    basis: "read",
    asked: "read",
    unit: "lines",
    count: CHANGES.length,
    canKeep: true,
    changes: CHANGES,
  };
}

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

function show(view: DocumentView): void {
  component = mount(CodeReview, { target, props: { view, workspace: 1 } });
  flushSync();
}

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 3000, interval: 10 });
}

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  review.state = { status: "idle" };
  vi.restoreAllMocks();
});

describe("a code file in review", () => {
  it("draws the changes of the version the review was drawn from", async () => {
    review.state = { status: "reviewing", view: reviewView("h1"), at: 0 };
    show(textView("h1"));
    await until(() => target.querySelectorAll(".rv-code-bar").length === 2, "the bars");
    expect(target.querySelector("[role=region]")?.getAttribute("aria-label")).toBe(
      "Code with changes",
    );
    expect([...target.querySelectorAll(".cm-line.rv-line")].map((l) => l.textContent)).toEqual([
      "    new();",
    ]);
    expect([...target.querySelectorAll(".rv-old-text")].map((l) => l.textContent)).toEqual([
      "    old();",
      "    // gone",
    ]);
    expect(target.querySelector(".rv-code-bar.is-current")).not.toBeNull();
  });

  it("draws nothing for a version of the file that is not the one the review was drawn from", async () => {
    review.state = { status: "reviewing", view: reviewView("other"), at: 0 };
    show(textView("h1"));
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(target.querySelectorAll(".rv-code-bar")).toHaveLength(0);
    expect(target.querySelector(".cm-editor")).not.toBeNull();
    // the review looks again at the new version: now it is drawn
    review.state = { status: "reviewing", view: reviewView("h1"), at: 0 };
    flushSync();
    await until(() => target.querySelectorAll(".rv-code-bar").length === 2, "the bars");
  });

  it("hands a click on Keep or Revert to the review", async () => {
    const keep = vi.spyOn(review, "keep").mockResolvedValue();
    const revert = vi.spyOn(review, "revert").mockResolvedValue();
    review.state = { status: "reviewing", view: reviewView("h1"), at: 0 };
    show(textView("h1"));
    await until(() => target.querySelectorAll(".rv-code-bar").length === 2, "the bars");
    const buttons = target.querySelectorAll<HTMLButtonElement>('[data-rv-change="1"] button');
    buttons[0]?.click();
    buttons[1]?.click();
    expect(keep).toHaveBeenCalledWith(1);
    expect(revert).toHaveBeenCalledWith(1);
  });

  it("follows the change the keys are on", async () => {
    review.state = { status: "reviewing", view: reviewView("h1"), at: 0 };
    show(textView("h1"));
    await until(() => target.querySelectorAll(".rv-code-bar").length === 2, "the bars");
    review.state = { status: "reviewing", view: reviewView("h1"), at: 1 };
    flushSync();
    await until(
      () => target.querySelectorAll(".rv-code-bar")[1]?.classList.contains("is-current") === true,
      "the second one is current",
    );
    expect(target.querySelectorAll(".rv-code-bar")[0]?.classList.contains("is-current")).toBe(
      false,
    );
  });
});
