import { describe, expect, it } from "vitest";
import { isStacked } from "./toastLayout";
import type { ToastModel } from "./stores/toast.svelte";

const message = (extra: Partial<Extract<ToastModel, { type: "message" }>> = {}): ToastModel => ({
  id: 1,
  type: "message",
  text: "Something happened",
  tone: "info",
  ...extra,
});

describe("isStacked", () => {
  const action = { label: "Show in file manager", run: () => {} };

  it("stacks a message with one button, whatever the button says", () => {
    expect(isStacked(message({ action }))).toBe(true);
    expect(isStacked(message({ action: { label: "Undo", run: () => {} } }))).toBe(true);
    expect(isStacked(message({ action, tone: "warn" }))).toBe(true);
  });

  it("stacks a message with two buttons", () => {
    expect(isStacked(message({ action, second: { label: "Undo", run: () => {} } }))).toBe(true);
  });

  it("keeps a message with no button on one row", () => {
    expect(isStacked(message())).toBe(false);
    expect(isStacked(message({ tone: "warn" }))).toBe(false);
  });

  it("keeps a change and an update on their own layout", () => {
    expect(
      isStacked({ id: 2, type: "change", count: 1, path: "a.md", kind: "modified", atMs: 0 }),
    ).toBe(false);
    expect(isStacked({ id: 3, type: "update" })).toBe(false);
  });
});
