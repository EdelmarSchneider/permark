import { describe, expect, it } from "vitest";
import { normalizeRightTab, stepTab, toggleOutline, type PanelView } from "./rightPanel";

describe("normalizeRightTab", () => {
  it("keeps the views this build knows", () => {
    expect(normalizeRightTab("changes")).toBe("changes");
    expect(normalizeRightTab("outline")).toBe("outline");
    expect(normalizeRightTab("notes")).toBe("notes");
  });

  it("reads anything else as Changes: a missing value, a newer view, a wrong type", () => {
    for (const value of [undefined, null, "", "commits", "Outline", 3, true, {}, ["outline"]]) {
      expect(normalizeRightTab(value)).toBe("changes");
    }
  });
});

describe("stepTab", () => {
  it("moves to the neighbour and wraps around", () => {
    expect(stepTab("changes", 1)).toBe("outline");
    expect(stepTab("outline", 1)).toBe("notes");
    expect(stepTab("notes", 1)).toBe("changes");
    expect(stepTab("changes", -1)).toBe("notes");
    expect(stepTab("notes", -1)).toBe("outline");
    expect(stepTab("outline", -1)).toBe("changes");
  });
});

describe("toggleOutline (Ctrl+Alt+O)", () => {
  const view = (open: boolean, tab: PanelView["tab"], back: PanelView["back"] = "changes") => ({
    open,
    tab,
    back,
  });

  it("shows the Outline from Changes and remembers where it came from", () => {
    expect(toggleOutline(view(true, "changes"))).toEqual(view(true, "outline", "changes"));
  });

  it("opens a collapsed panel on the Outline", () => {
    expect(toggleOutline(view(false, "changes"))).toEqual(view(true, "outline", "changes"));
  });

  it("opens a collapsed panel that already had the Outline selected, without losing the way back", () => {
    expect(toggleOutline(view(false, "outline", "changes"))).toEqual(
      view(true, "outline", "changes"),
    );
  });

  it("pressed again, returns to the previous view", () => {
    const shown = toggleOutline(view(true, "changes"));
    expect(toggleOutline(shown)).toEqual(view(true, "changes", "changes"));
  });

  it("goes back to Changes when there is nothing else to go back to", () => {
    // a panel restored on the Outline: the way back was never recorded
    expect(toggleOutline(view(true, "outline", "outline"))).toEqual(
      view(true, "changes", "changes"),
    );
  });

  it("alternates on every press", () => {
    let state = view(true, "changes");
    const seen: string[] = [];
    for (let i = 0; i < 4; i++) {
      state = toggleOutline(state);
      seen.push(state.tab);
    }
    expect(seen).toEqual(["outline", "changes", "outline", "changes"]);
  });
});
