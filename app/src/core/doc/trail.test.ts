import { describe, expect, it } from "vitest";
import { Trail } from "./trail";

const at = (path: string, anchor: string | null = null) => ({ path, anchor });

describe("Trail", () => {
  it("starts empty and cannot move", () => {
    const trail = new Trail();
    expect(trail.here).toBeNull();
    expect(trail.canBack).toBe(false);
    expect(trail.canForward).toBe(false);
    expect(trail.back()).toBeNull();
    expect(trail.forward()).toBeNull();
    expect(trail.jump(0)).toBeNull();
  });

  it("goes back and forward over what was visited", () => {
    const trail = new Trail();
    for (const p of ["a.md", "b.md", "c.md"]) trail.visit(at(p));
    expect(trail.here).toEqual(at("c.md"));
    expect(trail.back()).toEqual(at("b.md"));
    expect(trail.back()).toEqual(at("a.md"));
    expect(trail.back()).toBeNull();
    expect(trail.forward()).toEqual(at("b.md"));
    expect(trail.canForward).toBe(true);
  });

  it("drops what was ahead on a new visit", () => {
    const trail = new Trail();
    for (const p of ["a.md", "b.md", "c.md"]) trail.visit(at(p));
    trail.back();
    trail.visit(at("d.md"));
    expect(trail.canForward).toBe(false);
    expect(trail.back()).toEqual(at("b.md"));
  });

  it("counts the same place once, but another anchor of it as a new place", () => {
    const trail = new Trail();
    trail.visit(at("a.md"));
    trail.visit(at("a.md"));
    expect(trail.canBack).toBe(false);
    trail.visit(at("a.md", "intro"));
    expect(trail.canBack).toBe(true);
    expect(trail.back()).toEqual(at("a.md"));
  });

  it("lists the last places newest first and jumps to one", () => {
    const trail = new Trail();
    for (let i = 0; i < 40; i++) trail.visit(at(`d${i}.md`));
    const menu = trail.recent();
    expect(menu).toHaveLength(30);
    expect(menu[0]).toEqual({ index: 39, place: at("d39.md"), current: true });
    expect(menu[29]?.place).toEqual(at("d10.md"));
    expect(trail.jump(10)).toEqual(at("d10.md"));
    expect(trail.recent()[0]?.current).toBe(false);
    expect(trail.canForward).toBe(true);
    expect(trail.jump(-1)).toBeNull();
    expect(trail.jump(40)).toBeNull();
  });

  it("keeps a bounded number of places", () => {
    const trail = new Trail();
    for (let i = 0; i < 500; i++) trail.visit(at(`d${i}.md`));
    expect(trail.recent(1000)).toHaveLength(100);
    expect(trail.here).toEqual(at("d499.md"));
    trail.clear();
    expect(trail.here).toBeNull();
  });
});
