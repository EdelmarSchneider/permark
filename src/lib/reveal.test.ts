import { describe, expect, it } from "vitest";
import { nearestScrollTop, planReveal, shouldAutoReveal } from "./reveal";

describe("planReveal", () => {
  it("lists the folders above the file, outermost first, and targets the file", () => {
    const plan = planReveal("docs/specs/live-feed.md", new Set());
    expect(plan.target).toBe("docs/specs/live-feed.md");
    expect(plan.expand).toEqual(["docs", "docs/specs"]);
    expect(plan.section).toBe("vault");
  });

  it("leaves out the folders that are open already", () => {
    expect(planReveal("docs/specs/a.md", new Set(["docs"])).expand).toEqual(["docs/specs"]);
    expect(planReveal("docs/specs/a.md", new Set(["docs", "docs/specs"])).expand).toEqual([]);
    // an open inner folder does not make its closed parent visible
    expect(planReveal("docs/specs/a.md", new Set(["docs/specs"])).expand).toEqual(["docs"]);
  });

  it("opens nothing for a file at the project root", () => {
    const plan = planReveal("README.md", new Set());
    expect(plan.expand).toEqual([]);
    expect(plan.target).toBe("README.md");
    expect(plan.section).toBe("vault");
  });

  it("reveals a worktree file under its worktree root, in the worktrees section", () => {
    const plan = planReveal("@wt/feature-ui/docs/ui-notes.md", new Set());
    expect(plan.expand).toEqual(["@wt/feature-ui", "@wt/feature-ui/docs"]);
    expect(plan.section).toBe("worktrees");
    expect(planReveal("@wt/feature-ui/README.md", new Set()).expand).toEqual(["@wt/feature-ui"]);
    expect(planReveal("@wt/feature-ui/README.md", new Set(["@wt/feature-ui"])).expand).toEqual([]);
  });

  it("does not take a folder that merely looks like a worktree for one", () => {
    const plan = planReveal("wt/docs/a.md", new Set());
    expect(plan.expand).toEqual(["wt", "wt/docs"]);
    expect(plan.section).toBe("vault");
  });
});

describe("shouldAutoReveal", () => {
  const opened = { autoReveal: true, simple: false, movedWithin: false };

  it("reveals a newly opened document by default", () => {
    expect(shouldAutoReveal(opened)).toBe(true);
  });

  it("does nothing when the person turned the setting off", () => {
    expect(shouldAutoReveal({ ...opened, autoReveal: false })).toBe(false);
  });

  it("does nothing in Simple mode, which has no Explorer", () => {
    expect(shouldAutoReveal({ ...opened, simple: true })).toBe(false);
  });

  it("does not follow a jump inside the document that is already open", () => {
    expect(shouldAutoReveal({ ...opened, movedWithin: true })).toBe(false);
  });
});

describe("nearestScrollTop", () => {
  const view = { scrollTop: 100, height: 200 }; // shows 100..300

  it("does not move when the row is fully visible", () => {
    expect(nearestScrollTop({ top: 150, bottom: 174 }, view)).toBeNull();
    expect(nearestScrollTop({ top: 100, bottom: 124 }, view)).toBeNull();
    expect(nearestScrollTop({ top: 276, bottom: 300 }, view)).toBeNull();
  });

  it("scrolls up just enough to show a row above the view", () => {
    expect(nearestScrollTop({ top: 40, bottom: 64 }, view)).toBe(40);
    expect(nearestScrollTop({ top: 40, bottom: 64 }, view, 8)).toBe(32);
    expect(nearestScrollTop({ top: 3, bottom: 27 }, view, 8)).toBe(0);
  });

  it("scrolls down just enough to show a row below the view", () => {
    expect(nearestScrollTop({ top: 500, bottom: 524 }, view)).toBe(324);
    expect(nearestScrollTop({ top: 500, bottom: 524 }, view, 8)).toBe(332);
  });

  it("counts a half-visible row as hidden", () => {
    expect(nearestScrollTop({ top: 290, bottom: 314 }, view)).toBe(114);
    expect(nearestScrollTop({ top: 90, bottom: 114 }, view)).toBe(90);
  });
});
