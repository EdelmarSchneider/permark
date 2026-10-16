import { describe, expect, it } from "vitest";
import {
  normalizeRightTab,
  reopenButtonShown,
  reopenDotShown,
  stepTab,
  toggleOutline,
  type PanelView,
} from "./rightPanel";

describe("normalizeRightTab", () => {
  it("keeps the views this build knows", () => {
    expect(normalizeRightTab("changes")).toBe("changes");
    expect(normalizeRightTab("outline")).toBe("outline");
    expect(normalizeRightTab("comments")).toBe("comments");
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
    expect(stepTab("outline", 1)).toBe("comments");
    expect(stepTab("comments", 1)).toBe("changes");
    expect(stepTab("changes", -1)).toBe("comments");
    expect(stepTab("comments", -1)).toBe("outline");
    expect(stepTab("outline", -1)).toBe("changes");
  });
});

describe("reopenButtonShown (the corner button that brings a collapsed panel back)", () => {
  it("shows while the panel is collapsed", () => {
    expect(reopenButtonShown({ simple: false, open: false })).toBe(true);
  });

  it("hides while the panel is open: its own button collapses it", () => {
    expect(reopenButtonShown({ simple: false, open: true })).toBe(false);
  });

  it("never shows in Simple mode, which has no panel to bring back", () => {
    expect(reopenButtonShown({ simple: true, open: false })).toBe(false);
    expect(reopenButtonShown({ simple: true, open: true })).toBe(false);
  });
});

describe("reopenDotShown (new changes while the panel is collapsed)", () => {
  it("shows the dot when something new arrived and the button is there", () => {
    expect(reopenDotShown({ simple: false, open: false, notify: true })).toBe(true);
  });

  it("stays off when nothing new arrived", () => {
    expect(reopenDotShown({ simple: false, open: false, notify: false })).toBe(false);
  });

  it("has no dot without the button: the panel is open, or this is Simple mode", () => {
    expect(reopenDotShown({ simple: false, open: true, notify: true })).toBe(false);
    expect(reopenDotShown({ simple: true, open: false, notify: true })).toBe(false);
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
