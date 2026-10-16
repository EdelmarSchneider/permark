import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "./toast.svelte";

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  toast.dismiss();
  vi.runAllTimers();
  vi.useRealTimers();
});

describe("the buttons of a message", () => {
  it("run the one that was pressed, and then the toast goes", () => {
    const undo = vi.fn();
    const open = vi.fn();
    toast.message(
      "Moved",
      "info",
      { label: "Open in project", run: open },
      { label: "Undo", run: undo },
    );
    expect(toast.current).toMatchObject({
      text: "Moved",
      action: { label: "Open in project" },
      second: { label: "Undo" },
    });
    toast.runAction("second");
    expect(undo).toHaveBeenCalledTimes(1);
    expect(open).not.toHaveBeenCalled();
    expect(toast.leaving).toBe(true);
  });

  it("run the first one by default, and a message with no second button has none", () => {
    const undo = vi.fn();
    toast.message("Deleted", "info", { label: "Undo", run: undo });
    expect(toast.current).toMatchObject({ second: undefined });
    toast.runAction();
    expect(undo).toHaveBeenCalledTimes(1);
    // asking for a button that is not there runs nothing
    toast.message("Deleted", "info", { label: "Undo", run: undo });
    toast.runAction("second");
    expect(undo).toHaveBeenCalledTimes(1);
  });
});
