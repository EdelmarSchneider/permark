// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../../api";
import { BlockHost, type BlockHooks } from "./blocks";

function block(id: string, html: string | null = `<p>${id}</p>`): Block {
  return { id, kind: "p", lineStart: 1, lineEnd: 1, html };
}

type Rig = {
  container: HTMLElement;
  scroller: HTMLElement;
  host: BlockHost;
  hooks: BlockHooks & { enhanced: string[]; seen: string[]; undone: string[] };
  scrollTop: { value: number };
};

function rig(slices = { first: 2, next: 2 }): Rig {
  const scroller = document.createElement("div");
  const container = document.createElement("div");
  scroller.append(container);
  document.body.append(scroller);
  const scrollTop = { value: 0 };
  Object.defineProperty(scroller, "scrollTop", {
    get: () => scrollTop.value,
    set: (v: number) => {
      scrollTop.value = v;
    },
  });
  const log = { enhanced: [] as string[], seen: [] as string[], undone: [] as string[] };
  const hooks = {
    ...log,
    enhance(element: HTMLElement) {
      log.enhanced.push(element.textContent ?? "");
      return element;
    },
    near(element: HTMLElement) {
      const id = element.getAttribute("data-b") ?? "";
      log.seen.push(id);
      return () => log.undone.push(id);
    },
  };
  const host = new BlockHost(slices);
  host.attach(container, scroller, hooks);
  return { container, scroller, host, hooks, scrollTop };
}

/** A made-up layout: every block 100 px high, stacked in DOM order, shifted by the pane's scroll position. */
function fakeLayout(r: Rig): () => void {
  const original = HTMLElement.prototype.getBoundingClientRect;
  HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
    const box = (top: number, height: number): DOMRect => ({
      top,
      bottom: top + height,
      left: 0,
      right: 100,
      width: 100,
      height,
      x: 0,
      y: top,
      toJSON: () => ({}),
    });
    if (this === r.scroller) return box(0, 600);
    const at = Array.from(r.container.children).indexOf(this);
    return box(at * 100 - r.scrollTop.value, 100);
  };
  return () => {
    HTMLElement.prototype.getBoundingClientRect = original;
  };
}

const ids = (r: Rig): string[] =>
  Array.from(r.container.children).map((c) => c.getAttribute("data-b") ?? "");

beforeEach(() => {
  document.body.replaceChildren();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("BlockHost", () => {
  it("builds a fresh document in order, marks blocks and enhances each once", async () => {
    const r = rig();
    const applied = r.host.apply(
      [block("a"), block("b"), block("c"), block("d"), block("e")],
      true,
    );
    expect(ids(r)).toEqual(["a", "b"]);
    await applied.settled;
    expect(ids(r)).toEqual(["a", "b", "c", "d", "e"]);
    expect(r.host.ids()).toEqual(["a", "b", "c", "d", "e"]);
    expect(r.hooks.enhanced).toEqual(["a", "b", "c", "d", "e"]);
    expect(applied.missing).toEqual([]);
  });

  it("keeps the DOM node of every block whose id is unchanged and replaces the rest", () => {
    const r = rig({ first: 100, next: 100 });
    r.host.apply([block("a"), block("b"), block("c")], true);
    const [a, b, c] = Array.from(r.container.children);
    const applied = r.host.apply(
      [block("a", null), block("x"), block("c", null), block("y")],
      false,
    );
    expect(ids(r)).toEqual(["a", "x", "c", "y"]);
    expect(r.container.children[0]).toBe(a);
    expect(r.container.children[2]).toBe(c);
    expect(b?.isConnected).toBe(false);
    expect(applied.missing).toEqual([]);
    // b's deferred work was undone when it went
    expect(r.hooks.undone).toEqual(["b"]);
  });

  it("moves blocks that changed place without rebuilding them", () => {
    const r = rig({ first: 100, next: 100 });
    r.host.apply([block("a"), block("b"), block("c"), block("d")], true);
    const before = r.hooks.enhanced.length;
    r.host.apply([block("d", null), block("a", null), block("c", null), block("b", null)], false);
    expect(ids(r)).toEqual(["d", "a", "c", "b"]);
    expect(r.hooks.enhanced.length).toBe(before);
  });

  it("reports blocks it was told it holds but does not", () => {
    const r = rig({ first: 100, next: 100 });
    r.host.apply([block("a")], true);
    const applied = r.host.apply([block("a", null), block("ghost", null), block("b")], false);
    expect(applied.missing).toEqual(["ghost"]);
    expect(ids(r)).toEqual(["a", "b"]);
  });

  it("a new apply stops the slices of the old one and releases whoever waits", async () => {
    vi.useFakeTimers();
    const r = rig({ first: 1, next: 1 });
    const first = r.host.apply([block("a"), block("b"), block("c"), block("d")], true);
    const waiting = vi.fn();
    void first.settled.then(waiting);
    r.host.apply([block("z")], true);
    await vi.runAllTimersAsync();
    expect(waiting).toHaveBeenCalled();
    expect(ids(r)).toEqual(["z"]);
  });

  it("a fresh document replaces the old one entirely", () => {
    const r = rig({ first: 100, next: 100 });
    r.host.apply([block("a"), block("b")], true);
    r.host.apply([block("a"), block("q")], true);
    expect(ids(r)).toEqual(["a", "q"]);
    expect(r.hooks.enhanced).toEqual(["a", "b", "a", "q"]);
  });

  it("does its deferred work for blocks near the screen (no observer: at once) and undoes it on detach", () => {
    const r = rig({ first: 100, next: 100 });
    r.host.apply([block("a"), block("b")], true);
    expect(r.hooks.seen).toEqual(["a", "b"]);
    r.host.detach();
    expect(r.hooks.undone.sort()).toEqual(["a", "b"]);
    expect(r.host.ids()).toEqual([]);
    // nothing to do after detach
    expect(r.host.apply([block("c")], true).missing).toEqual([]);
  });

  it("keeps the reader's place when blocks above change height or appear", () => {
    const r = rig({ first: 100, next: 100 });
    const restore = fakeLayout(r);
    r.host.apply(
      ["a", "b", "c", "d", "e", "f"].map((i) => block(i)),
      true,
    );
    r.scrollTop.value = 250; // c is at -50..50, d at 50..150: c is the first block in view
    // two blocks appear above: c moves down by 200, the pane must follow
    r.host.apply(
      [block("n1"), block("n2"), ...["a", "b", "c", "d", "e", "f"].map((i) => block(i, null))],
      false,
    );
    expect(r.scrollTop.value).toBe(450);
    restore();
  });

  it("falls back to the nearest block above that survived when the first one in view was replaced", () => {
    const r = rig({ first: 100, next: 100 });
    const restore = fakeLayout(r);
    r.host.apply(
      ["a", "b", "c", "d", "e"].map((i) => block(i)),
      true,
    );
    r.scrollTop.value = 250; // c first in view; b just above it
    // c is edited (new id), a new block appears at the top: b is the survivor to hold on to
    r.host.apply(
      [
        block("n"),
        block("a", null),
        block("b", null),
        block("c2"),
        block("d", null),
        block("e", null),
      ],
      false,
    );
    // b sat at 100 - 250 = -150; now it is at index 2 -> 200 - 250 = -50: the pane moves down 100 to put b back at -150
    expect(r.scrollTop.value).toBe(350);
    restore();
  });

  it("finds anchors by id attribute, odd characters included", () => {
    const r = rig({ first: 100, next: 100 });
    r.host.apply(
      [block("a", '<h2 id="intro">Intro</h2>'), block("b", '<h2 id="a&quot;b">q</h2>')],
      true,
    );
    expect(r.host.byAnchor("intro")?.textContent).toBe("Intro");
    expect(r.host.byAnchor('a"b')?.textContent).toBe("q");
    expect(r.host.byAnchor("nope")).toBeNull();
    expect(r.host.byAnchor("")).toBeNull();
    expect(r.host.byAnchor('x"] , [id="intro')).toBeNull();
  });
});
