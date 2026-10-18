import { describe, expect, it } from "vitest";
import { DOC_MODES, focusTarget, otherEditMode, shortcutToward } from "./editMode";

describe("the modes of the header switch", () => {
  it("are View, Edit and Source, stored as read, visual and source, in that order", () => {
    expect([...DOC_MODES]).toEqual(["read", "visual", "source"]);
  });

  it("have an other editor to go to", () => {
    expect(otherEditMode("visual")).toBe("source");
    expect(otherEditMode("source")).toBe("visual");
  });
});

describe("shortcutToward", () => {
  it("names Ctrl+E for going back to reading from either editor", () => {
    expect(shortcutToward("read", "visual", "visual")).toBe("Ctrl+E");
    expect(shortcutToward("read", "source", "source")).toBe("Ctrl+E");
  });

  it("names Ctrl+Alt+E for the other editor", () => {
    expect(shortcutToward("source", "visual", "visual")).toBe("Ctrl+Alt+E");
    expect(shortcutToward("visual", "source", "source")).toBe("Ctrl+Alt+E");
  });

  it("from reading: Ctrl+E for the editor used last, Ctrl+Alt+E for the other one", () => {
    expect(shortcutToward("visual", "read", "visual")).toBe("Ctrl+E");
    expect(shortcutToward("source", "read", "visual")).toBe("Ctrl+Alt+E");
    expect(shortcutToward("source", "read", "source")).toBe("Ctrl+E");
    expect(shortcutToward("visual", "read", "source")).toBe("Ctrl+Alt+E");
  });

  it("has none for the mode already shown", () => {
    for (const mode of DOC_MODES) expect(shortcutToward(mode, mode, "visual")).toBeNull();
  });
});

describe("focusTarget", () => {
  it("moves the focus along the segments with the arrow keys, around the ends", () => {
    expect(focusTarget("read", "ArrowRight")).toBe("visual");
    expect(focusTarget("visual", "ArrowRight")).toBe("source");
    expect(focusTarget("source", "ArrowRight")).toBe("read");
    expect(focusTarget("read", "ArrowLeft")).toBe("source");
    expect(focusTarget("source", "ArrowLeft")).toBe("visual");
    expect(focusTarget("visual", "ArrowDown")).toBe("source");
    expect(focusTarget("visual", "ArrowUp")).toBe("read");
  });

  it("goes to the first and the last segment with Home and End", () => {
    expect(focusTarget("visual", "Home")).toBe("read");
    expect(focusTarget("visual", "End")).toBe("source");
  });

  it("leaves every other key alone", () => {
    for (const key of ["Enter", " ", "Tab", "e", "Escape"]) {
      expect(focusTarget("visual", key)).toBeNull();
    }
  });
});
