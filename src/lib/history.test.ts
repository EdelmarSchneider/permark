import { describe, expect, it } from "vitest";
import { NavHistory } from "./history";

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

  it("caps its length", () => {
    const h = new NavHistory();
    for (let i = 0; i < 150; i++) h.push(`f${i}.md`);
    let steps = 0;
    while (h.back()) steps++;
    expect(steps).toBe(99);
  });
});
