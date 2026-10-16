import { describe, expect, it } from "vitest";
import { NavHistory, TRAIL_MENU_LIMIT } from "./history";

describe("NavHistory", () => {
  it("starts empty with nothing to go back or forward to", () => {
    const h = new NavHistory();
    expect(h.current).toBeNull();
    expect(h.canBack).toBe(false);
    expect(h.canForward).toBe(false);
    expect(h.back()).toBeNull();
    expect(h.forward()).toBeNull();
  });

  it("goes back and forward through visited documents", () => {
    const h = new NavHistory();
    h.push("a.md");
    h.push("b.md");
    h.push("c.md");
    expect(h.back()?.path).toBe("b.md");
    expect(h.back()?.path).toBe("a.md");
    expect(h.canBack).toBe(false);
    expect(h.forward()?.path).toBe("b.md");
    expect(h.canForward).toBe(true);
  });

  it("drops forward entries when navigating from the middle", () => {
    const h = new NavHistory();
    h.push("a.md");
    h.push("b.md");
    h.back();
    h.push("c.md");
    expect(h.canForward).toBe(false);
    expect(h.back()?.path).toBe("a.md");
  });

  it("ignores a push of the current path", () => {
    const h = new NavHistory();
    h.push("a.md");
    h.push("a.md");
    expect(h.canBack).toBe(false);
  });

  it("restores the saved scroll position of an entry", () => {
    const h = new NavHistory();
    h.push("a.md");
    h.saveScroll(420);
    h.push("b.md");
    expect(h.back()).toEqual({ path: "a.md", scrollTop: 420 });
  });

  it("replaces the current entry", () => {
    const h = new NavHistory();
    h.push("a.md");
    h.push("b.md");
    h.replaceCurrent("c.md");
    expect(h.current?.path).toBe("c.md");
    expect(h.back()?.path).toBe("a.md");
  });

  it("lists recent paths without duplicates, newest first", () => {
    const h = new NavHistory();
    ["a.md", "b.md", "a.md", "c.md"].forEach((p) => h.push(p));
    expect(h.recentPaths()).toEqual(["c.md", "a.md", "b.md"]);
  });

  it("follows moved and renamed files, and the folders they are in", () => {
    const h = new NavHistory();
    ["docs/a.md", "docs/sub/b.md", "README.md"].forEach((p) => h.push(p));
    h.rename([
      { from: "docs", to: "manual" },
      { from: "README.md", to: "intro.md" },
    ]);
    expect(h.recentPaths()).toEqual(["intro.md", "manual/sub/b.md", "manual/a.md"]);
    // the position in the history stays
    expect(h.back()?.path).toBe("manual/sub/b.md");
  });

  describe("the trail", () => {
    /** a, b, c, d, e visited in that order, with the reader back on `at`. */
    const visited = (at: string): NavHistory => {
      const h = new NavHistory();
      ["a.md", "b.md", "c.md", "d.md", "e.md"].forEach((p) => h.push(p));
      while (h.current?.path !== at) h.back();
      return h;
    };

    it("jumps several steps back or forward at once, like a browser's go(n)", () => {
      const h = visited("e.md");
      expect(h.go(-3)?.path).toBe("b.md");
      expect(h.canBack).toBe(true);
      expect(h.canForward).toBe(true);
      expect(h.go(2)?.path).toBe("d.md");
      expect(h.go(-1)?.path).toBe("c.md");
      // one step is Back and Forward
      expect(h.go(1)?.path).toBe("d.md");
    });

    it("keeps every entry after a jump, so Back and Forward walk the same trail", () => {
      const h = visited("e.md");
      h.go(-3);
      expect(h.stops("forward").map((s) => s.path)).toEqual(["c.md", "d.md", "e.md"]);
      expect(h.stops("back").map((s) => s.path)).toEqual(["a.md"]);
      expect(h.forward()?.path).toBe("c.md");
      expect(h.back()?.path).toBe("b.md");
      expect(h.back()?.path).toBe("a.md");
    });

    it("does not move for a jump past either end, or for no jump at all", () => {
      const h = visited("c.md");
      expect(h.go(3)).toBeNull();
      expect(h.go(-3)).toBeNull();
      expect(h.go(0)).toBeNull();
      expect(h.go(0.5)).toBeNull();
      expect(h.go(Number.NaN)).toBeNull();
      expect(h.current?.path).toBe("c.md");
      expect(h.go(2)?.path).toBe("e.md");
      expect(h.go(-4)?.path).toBe("a.md");
    });

    it("has nowhere to jump in an empty history", () => {
      const h = new NavHistory();
      expect(h.go(-1)).toBeNull();
      expect(h.go(1)).toBeNull();
      expect(h.stops("back")).toEqual([]);
      expect(h.stops("forward")).toEqual([]);
      expect(h.trail()).toEqual([]);
    });

    it("restores the scroll position of the entry it jumps to", () => {
      const h = new NavHistory();
      h.push("a.md");
      h.saveScroll(120);
      h.push("b.md");
      h.saveScroll(300);
      h.push("c.md");
      expect(h.go(-2)).toEqual({ path: "a.md", scrollTop: 120 });
      expect(h.go(1)).toEqual({ path: "b.md", scrollTop: 300 });
    });

    it("lists the way back nearest first, each stop with the jump that reaches it", () => {
      const h = visited("d.md");
      expect(h.stops("back")).toEqual([
        { offset: -1, path: "c.md", current: false },
        { offset: -2, path: "b.md", current: false },
        { offset: -3, path: "a.md", current: false },
      ]);
      expect(h.stops("forward")).toEqual([{ offset: 1, path: "e.md", current: false }]);
    });

    it("jumps to the stop it listed", () => {
      const h = visited("d.md");
      const stop = h.stops("back")[1];
      expect(h.go(stop.offset)?.path).toBe(stop.path);
    });

    it("limits how many stops a menu lists", () => {
      const h = new NavHistory();
      for (let i = 0; i < 40; i++) h.push(`f${i}.md`);
      expect(h.stops("back")).toHaveLength(TRAIL_MENU_LIMIT);
      expect(h.stops("back", 3).map((s) => s.path)).toEqual(["f38.md", "f37.md", "f36.md"]);
      expect(h.stops("forward")).toEqual([]);
    });

    it("keeps a document that was visited twice as two stops", () => {
      const h = new NavHistory();
      ["a.md", "b.md", "a.md", "c.md"].forEach((p) => h.push(p));
      expect(h.stops("back").map((s) => [s.offset, s.path])).toEqual([
        [-1, "a.md"],
        [-2, "b.md"],
        [-3, "a.md"],
      ]);
      expect(h.go(-1)?.path).toBe("a.md");
      expect(h.go(-1)?.path).toBe("b.md");
    });

    it("lists the whole trail newest first with the current document marked", () => {
      const h = visited("c.md");
      expect(h.trail()).toEqual([
        { offset: 2, path: "e.md", current: false },
        { offset: 1, path: "d.md", current: false },
        { offset: 0, path: "c.md", current: true },
        { offset: -1, path: "b.md", current: false },
        { offset: -2, path: "a.md", current: false },
      ]);
    });

    it("follows renamed files in the trail", () => {
      const h = visited("c.md");
      h.rename([{ from: "b.md", to: "docs/b.md" }]);
      expect(h.stops("back")[0]).toEqual({ offset: -1, path: "docs/b.md", current: false });
    });
  });

  it("caps its length", () => {
    const h = new NavHistory();
    for (let i = 0; i < 150; i++) h.push(`f${i}.md`);
    let steps = 0;
    while (h.back()) steps++;
    expect(steps).toBe(99);
  });
});
